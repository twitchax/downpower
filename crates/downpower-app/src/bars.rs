//! Bar geometry for the trends page (PRD-0006 T-004), apart from the view so it tests natively: one
//! column per year or calendar month across the stacked area's stretched viewBox
//! ([`WIDTH`] × [`HEIGHT`]), values stacked bottom up, and a box from one value to another with a
//! line across it for a spread.

use crate::area::{HEIGHT, WIDTH, y};

/// The share of a column's width a bar fills.
const FILL: f64 = 0.7;

/// The share of a column's width a spread's box fills.
const SPREAD: f64 = 0.4;

/// A rectangle in the viewBox.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One stacked segment: its series' key, its column and its rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment<K> {
    pub key: K,
    pub column: usize,
    pub rect: Rect,
}

/// The left edge and width of a mark filling `fill` of column `index` of `count`, centered.
fn mark(index: usize, count: usize, fill: f64) -> (f64, f64) {
    #[allow(clippy::cast_precision_loss, reason = "a few dozen columns")]
    let (index, count) = (index as f64, count.max(1) as f64);
    let column = WIDTH / count;
    let width = column * fill;
    (index * column + (column - width) / 2.0, width)
}

/// Column `index` of `count`'s whole width, for a hover band: its left edge and width.
#[must_use]
pub fn column(index: usize, count: usize) -> (f64, f64) {
    mark(index, count, 1.0)
}

/// The center of column `index` of `count`, as a fraction of the width.
#[must_use]
pub fn center(index: usize, count: usize) -> f64 {
    let (x, width) = column(index, count);
    (x + width / 2.0) / WIDTH
}

/// The tallest column, summing the values at `indexes`.
#[must_use]
pub fn peak<C: AsRef<[f64]>>(columns: &[C], indexes: &[usize]) -> f64 {
    columns
        .iter()
        .map(|column| indexes.iter().filter_map(|index| column.as_ref().get(*index)).sum::<f64>())
        .fold(0.0, f64::max)
}

/// `columns` stacked bottom up in `series` order (each a key and the index of its value in a
/// column), on a scale topping out at `top`. A zero value draws nothing.
#[must_use]
pub fn stacked<K: Copy, C: AsRef<[f64]>>(columns: &[C], series: &[(K, usize)], top: f64) -> Vec<Segment<K>> {
    let mut segments = Vec::new();
    for (index, values) in columns.iter().enumerate() {
        let (x, width) = mark(index, columns.len(), FILL);
        let mut floor = 0.0;
        for (key, at) in series {
            let value = values.as_ref().get(*at).copied().unwrap_or(0.0);
            if value <= 0.0 {
                continue;
            }
            let (lower, upper) = (y(floor, top), y(floor + value, top));
            floor += value;
            segments.push(Segment {
                key: *key,
                column: index,
                rect: Rect {
                    x,
                    y: upper,
                    width,
                    height: (lower - upper).clamp(0.0, HEIGHT),
                },
            });
        }
    }
    segments
}

/// A spread in column `index` of `count`: a box from `low` to `high`, and the path of a line
/// across it at `middle`, on a scale topping out at `top`.
#[must_use]
pub fn spread(index: usize, count: usize, (low, high): (f64, f64), middle: f64, top: f64) -> (Rect, String) {
    let (x, width) = mark(index, count, SPREAD);
    let (lower, upper) = (y(low, top), y(high, top));
    let rect = Rect {
        x,
        y: upper,
        width,
        height: (lower - upper).max(0.0),
    };
    (rect, format!("M{x:.1},{:.2}H{:.1}", y(middle, top), x + width))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn bars_sit_centered_in_their_columns() {
        // Four columns of 250: a bar fills 175 of its column, 37.5 in from each side.
        let (x, width) = mark(1, 4, FILL);
        assert!(near(x, 287.5) && near(width, 175.0));
        assert_eq!(column(3, 4), (750.0, 250.0));
        assert!(near(center(0, 4), 0.125));
        assert!(near(center(3, 4), 0.875));
    }

    #[test]
    fn stacked_segments_rise_from_the_baseline_in_series_order() {
        let columns = [[1.0, 3.0], [0.0, 2.0]];
        let segments = stacked(&columns, &[("b", 1), ("a", 0)], 4.0);
        let summary: Vec<(&str, usize, f64, f64)> = segments.iter().map(|segment| (segment.key, segment.column, segment.rect.y, segment.rect.height)).collect();
        // The scale tops at 4 over 240: 60 per unit. "b" sits on the baseline, "a" on top of it, and
        // the second column's zero "a" draws nothing.
        assert_eq!(summary, [("b", 0, 60.0, 180.0), ("a", 0, 0.0, 60.0), ("b", 1, 120.0, 120.0)]);
        assert!(near(segments[0].rect.width, 350.0));
        assert!(near(peak(&columns, &[0, 1]), 4.0));
        assert!(near(peak(&columns, &[0]), 1.0));
    }

    #[test]
    fn a_spread_is_a_narrow_box_with_a_line_across_it() {
        let (rect, line) = spread(0, 2, (10.0, 30.0), 20.0, 40.0);
        // A column of 500, the box 200 wide and centered; 6 per unit on a scale of 40.
        assert_eq!(
            rect,
            Rect {
                x: 150.0,
                y: 60.0,
                width: 200.0,
                height: 120.0,
            }
        );
        assert_eq!(line, "M150.0,120.00H350.0");
    }
}
