//! The barcode's pixels, computed apart from the browser so they test natively (DESIGN.md
//! Decision 23). One row per unit and one device-pixel column per span of days. A column's color
//! is the visible family that lost the most in its span, and its opacity the mean depth of the cut.
//! A unit in service at full power shows a faint track, and an out-of-service span shows nothing.

use std::ops::Range;

use chrono::NaiveDate;

use downpower_core::Families;
use downpower_core::barcode::{self, Barcode, Mark};

/// The days shown: `days` days from index `start` of the barcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: u32,
    pub days: u32,
}

impl Window {
    /// The days from `from` to `to`, clamped to what `barcode` covers. An empty or inverted range
    /// falls back to everything.
    #[must_use]
    pub fn new(barcode: &Barcode, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Self {
        let last = barcode.days().saturating_sub(1);
        let clamp = |date: NaiveDate| {
            let offset = (date - barcode.first()).num_days();
            u32::try_from(offset.max(0)).unwrap_or(u32::MAX).min(last)
        };
        let start = from.map_or(0, clamp);
        let end = to.map_or(last, clamp);
        if barcode.days() == 0 || end < start {
            return Self { start: 0, days: barcode.days() };
        }
        Self { start, days: end - start + 1 }
    }

    /// The barcode's day indices that device column `x` of `width` covers.
    #[must_use]
    pub fn span(self, x: u32, width: u32) -> Range<u32> {
        let span = barcode::span(x, width, self.days);
        let at = |index: usize| self.start + u32::try_from(index).unwrap_or(u32::MAX);
        at(span.start)..at(span.end)
    }
}

/// Device-pixel geometry: the canvas width, a row's pitch, and the gap left at the bottom of each
/// row so neighbors read apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub width: u32,
    pub pitch: u32,
    pub gap: u32,
}

/// Colors resolved from the stylesheet: each family's, the in-service track's, and the gray a
/// filtered-out family recedes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub families: [[u8; 3]; 8],
    pub track: [u8; 3],
    pub hidden: [u8; 3],
}

/// A mark's RGBA. A shallow cut stays visible: opacity runs from 0.3 at no depth to 1 at a full
/// outage.
#[must_use]
pub fn rgba(mark: Mark, palette: &Palette) -> [u8; 4] {
    let alpha = |depth: f32, floor: f32| {
        let alpha = (floor + (1.0 - floor) * depth.clamp(0.0, 1.0)) * 255.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0–255")]
        let alpha = alpha.round().clamp(0.0, 255.0) as u8;
        alpha
    };
    let [r, g, b] = match mark {
        Mark::Empty => return [0; 4],
        Mark::Full => palette.track,
        Mark::Reduced { family, .. } => palette.families[usize::from(family.index())],
        Mark::Hidden { .. } => palette.hidden,
    };
    let a = match mark {
        Mark::Reduced { depth, .. } => alpha(depth, 0.3),
        Mark::Hidden { depth } => alpha(depth, 0.2),
        _ => u8::MAX,
    };
    [r, g, b, a]
}

/// The barcode's RGBA pixels for `window`, `grid.width` wide and `grid.pitch` per row.
#[must_use]
pub fn paint(barcode: &Barcode, window: Window, grid: Grid, visible: Families, palette: &Palette) -> Vec<u8> {
    let width = grid.width as usize;
    let rows = barcode.units().len();
    let pitch = grid.pitch.max(1) as usize;
    let fill = pitch.saturating_sub(grid.gap as usize).max(1);
    let mut pixels = vec![0_u8; width * rows * pitch * 4];
    let mut line = vec![0_u8; width * 4];
    for row in 0..rows {
        let cells = barcode.row(row);
        for x in 0..grid.width {
            let span = window.span(x, grid.width);
            let range = span.start as usize..(span.end as usize).min(cells.len());
            let color = rgba(barcode::mark(cells.get(range).unwrap_or_default(), visible), palette);
            let at = x as usize * 4;
            line[at..at + 4].copy_from_slice(&color);
        }
        for y in 0..fill {
            let at = ((row * pitch) + y) * width * 4;
            pixels[at..at + width * 4].copy_from_slice(&line);
        }
    }
    pixels
}

/// The day a hover on `row`'s `days` lands on: the deepest cut in the span, or its middle day when
/// nothing is cut. `None` when no day in the span has data.
#[must_use]
pub fn deepest(barcode: &Barcode, row: usize, days: Range<u32>) -> Option<u32> {
    let cells: Vec<(u32, barcode::Cell)> = days.clone().filter_map(|index| Some((index, barcode.cell(row, index)?))).collect();
    let (index, cell) = cells
        .iter()
        .copied()
        .min_by_key(|(index, cell)| (cell.bucket, index.abs_diff(days.start + (days.end - days.start) / 2)))?;
    Some(if cell.bucket < barcode::FULL { index } else { cells[cells.len() / 2].0 })
}

#[cfg(test)]
mod tests {
    use anyhow::Context as _;
    use downpower_core::Family;
    use downpower_core::barcode::{Cell, NO_DATA, Packer, Unit};
    use pretty_assertions::assert_eq;

    use super::*;

    const PALETTE: Palette = Palette {
        families: [[1, 0, 0], [2, 0, 0], [3, 0, 0], [4, 0, 0], [5, 0, 0], [6, 0, 0], [7, 0, 0], [8, 0, 0]],
        track: [9, 9, 9],
        hidden: [7, 7, 7],
    };

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    /// Two units over four days from 2024-01-01: the first refueling on days 0–1 and full after, the
    /// second out of service until a forced cut on day 3.
    fn barcode() -> anyhow::Result<Barcode> {
        let mut packer = Packer::new(vec![
            Unit {
                docket: "05000275".to_owned(),
                name: "Diablo Canyon 1".to_owned(),
            },
            Unit {
                docket: "05000323".to_owned(),
                name: "Diablo Canyon 2".to_owned(),
            },
        ]);
        packer.set(0, date("2024-01-01"), Cell::new(Family::Refueling, 0))?;
        packer.set(0, date("2024-01-02"), Cell::new(Family::Refueling, 0))?;
        packer.set(0, date("2024-01-03"), Cell::new(Family::Unattributed, 100))?;
        packer.set(0, date("2024-01-04"), Cell::new(Family::Unattributed, 100))?;
        packer.set(1, date("2024-01-04"), Cell::new(Family::Forced, 50))?;
        packer.finish()?.context("cells were set")
    }

    #[test]
    fn a_window_clamps_to_the_data_and_ignores_an_inverted_range() -> anyhow::Result<()> {
        let barcode = barcode()?;
        assert_eq!(Window::new(&barcode, None, None), Window { start: 0, days: 4 });
        assert_eq!(Window::new(&barcode, Some(date("2024-01-02")), Some(date("2030-01-01"))), Window { start: 1, days: 3 });
        assert_eq!(Window::new(&barcode, Some(date("2020-01-01")), Some(date("2024-01-02"))), Window { start: 0, days: 2 });
        assert_eq!(Window::new(&barcode, Some(date("2024-01-04")), Some(date("2024-01-02"))), Window { start: 0, days: 4 });
        assert_eq!(Window { start: 1, days: 3 }.span(0, 3), 1..2);
        Ok(())
    }

    #[test]
    fn paint_colors_each_column_by_its_mark_and_leaves_the_row_gap_clear() -> anyhow::Result<()> {
        let barcode = barcode()?;
        let grid = Grid { width: 2, pitch: 2, gap: 1 };
        let pixels = paint(&barcode, Window::new(&barcode, None, None), grid, Families::ALL, &PALETTE);
        assert_eq!(pixels.len(), 2 * 2 * 2 * 4);
        let pixel = |x: usize, y: usize| pixels[(y * 2 + x) * 4..(y * 2 + x) * 4 + 4].to_vec();
        // Row 0: refueling at a full outage, then the full-power track.
        assert_eq!(pixel(0, 0), [1, 0, 0, 252]);
        assert_eq!(pixel(1, 0), [9, 9, 9, 255]);
        // The gap row under it is clear.
        assert_eq!(pixel(0, 1), [0, 0, 0, 0]);
        // Row 1: nothing in service, then a forced cut of half.
        assert_eq!(pixel(0, 2), [0, 0, 0, 0]);
        assert_eq!(pixel(1, 2)[..3], [3, 0, 0]);

        // With refueling filtered out, its outage recedes to the hidden gray.
        let filtered = paint(&barcode, Window::new(&barcode, None, None), grid, Families::ALL.without(Family::Refueling), &PALETTE);
        assert_eq!(filtered[..3], [7, 7, 7]);
        Ok(())
    }

    #[test]
    fn a_hover_lands_on_the_deepest_day_or_the_middle() -> anyhow::Result<()> {
        let barcode = barcode()?;
        assert_eq!(deepest(&barcode, 0, 1..4), Some(1));
        assert_eq!(deepest(&barcode, 0, 2..4), Some(3), "both full: the middle");
        assert_eq!(deepest(&barcode, 1, 0..3), None);
        assert_eq!(barcode.row(1)[0], NO_DATA);
        Ok(())
    }

    #[test]
    fn opacity_scales_with_depth() {
        let shallow = rgba(Mark::Reduced { family: Family::Grid, depth: 0.0 }, &PALETTE);
        let full = rgba(Mark::Reduced { family: Family::Grid, depth: 1.0 }, &PALETTE);
        assert_eq!((shallow[3], full[3]), (77, 255));
        assert_eq!(rgba(Mark::Empty, &PALETTE), [0; 4]);
    }
}
