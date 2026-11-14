//! The stacked area's geometry, apart from the view so it tests natively (DESIGN.md Decision 23):
//! months along x, reported power reduction stacked by family along y, in a fixed viewBox the SVG
//! stretches to its width. Only the paths live in that box; text sits in HTML beside it, so nothing
//! stretches with it.

use std::fmt::Write as _;

use downpower_core::{Families, Family};

/// The viewBox the paths are drawn in.
pub const WIDTH: f64 = 1000.0;
pub const HEIGHT: f64 = 240.0;

/// One family's band: its outline as an SVG path, and its top edge alone, where a surface-colored
/// seam separates it from the band above.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub family: Family,
    pub path: String,
    pub top: String,
}

/// Whether `months` months are few enough to draw seams. Past ten years a month is a pixel or two
/// wide, and seams along every step would darken the bands more than they separate them.
#[must_use]
pub const fn seamed(months: usize) -> bool {
    months <= 120
}

/// The months to draw, in terawatt-hours, each indexed by [`Family::index`].
pub type Months = [[f64; 8]];

/// The visible families stacked bottom up in [`Family::STACK`] order: per family, each month's
/// lower and upper edge. A hidden family is left out, so the visible ones sit on the baseline.
#[must_use]
pub fn stack(months: &Months, visible: Families) -> Vec<(Family, Vec<(f64, f64)>)> {
    let mut floor = vec![0.0; months.len()];
    visible
        .iter()
        .map(|family| {
            let edges = months
                .iter()
                .zip(floor.iter_mut())
                .map(|(month, floor)| {
                    let lower = *floor;
                    *floor += month[usize::from(family.index())];
                    (lower, *floor)
                })
                .collect();
            (family, edges)
        })
        .collect()
}

/// The tallest stacked month.
#[must_use]
pub fn peak(months: &Months, visible: Families) -> f64 {
    months
        .iter()
        .map(|month| visible.iter().map(|family| month[usize::from(family.index())]).sum::<f64>())
        .fold(0.0, f64::max)
}

/// The x of month `index` of `count`: each month is a step as wide as the box over the count.
#[must_use]
pub fn x(index: usize, count: usize) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "a few hundred months")]
    let x = index as f64 * WIDTH / count.max(1) as f64;
    x
}

/// The y of `value` on a scale topping out at `top`.
#[must_use]
pub fn y(value: f64, top: f64) -> f64 {
    if top <= 0.0 { HEIGHT } else { HEIGHT - value / top * HEIGHT }
}

/// The bands for `stack`, drawn as steps (each month holds its value across its width, since the
/// data is monthly totals), on a scale topping out at `top`.
#[must_use]
pub fn bands(stack: &[(Family, Vec<(f64, f64)>)], top: f64) -> Vec<Band> {
    stack
        .iter()
        .map(|(family, edges)| {
            let count = edges.len();
            let mut path = String::new();
            for (index, (_, upper)) in edges.iter().enumerate() {
                let verb = if index == 0 { 'M' } else { 'L' };
                let _ = write!(path, "{verb}{:.1},{:.2}L{:.1},{:.2}", x(index, count), y(*upper, top), x(index + 1, count), y(*upper, top));
            }
            let seam = path.clone();
            for (index, (lower, _)) in edges.iter().enumerate().rev() {
                let _ = write!(path, "L{:.1},{:.2}L{:.1},{:.2}", x(index + 1, count), y(*lower, top), x(index, count), y(*lower, top));
            }
            if !path.is_empty() {
                path.push('Z');
            }
            Band { family: *family, path, top: seam }
        })
        .collect()
}

/// A clean scale over `0..=peak`: the top tick and the ticks under it, at most `most` steps of 1,
/// 2 or 5 times a power of ten.
#[must_use]
pub fn ticks(peak: f64, most: usize) -> (f64, Vec<f64>) {
    if peak <= 0.0 || !peak.is_finite() {
        return (1.0, vec![0.0, 1.0]);
    }
    let rough = peak / f64::from(u32::try_from(most.max(1)).unwrap_or(u32::MAX));
    let magnitude = 10_f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * magnitude);
    let steps = (peak / step).ceil();
    let top = steps * step;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a handful of steps")]
    let ticks = (0..=steps as u32).map(|index| f64::from(index) * step).collect();
    (top, ticks)
}

/// The month under a pointer at `fraction` of the width.
#[must_use]
pub fn month_at(fraction: f64, count: usize) -> Option<usize> {
    if count == 0 || !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss, reason = "a few hundred months")]
    let index = (fraction * count as f64).floor() as usize;
    Some(index.min(count - 1))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn month(values: &[(Family, f64)]) -> [f64; 8] {
        let mut month = [0.0; 8];
        for (family, value) in values {
            month[usize::from(family.index())] = *value;
        }
        month
    }

    #[test]
    fn families_stack_in_order_and_hidden_ones_drop_out() {
        let months = [month(&[(Family::Refueling, 2.0), (Family::Forced, 1.0)]), month(&[(Family::Forced, 3.0)])];
        let stacked = stack(&months, Families::ALL);
        assert_eq!(stacked.len(), 8);
        assert_eq!(stacked[0], (Family::Refueling, vec![(0.0, 2.0), (0.0, 0.0)]));
        let forced = stacked.iter().find(|(family, _)| *family == Family::Forced).map(|(_, edges)| edges.clone());
        assert_eq!(forced, Some(vec![(2.0, 3.0), (0.0, 3.0)]), "forced sits on refueling");
        assert!((peak(&months, Families::ALL) - 3.0).abs() < 1e-9);

        let without = stack(&months, Families::ALL.without(Family::Refueling));
        let forced = without.iter().find(|(family, _)| *family == Family::Forced).map(|(_, edges)| edges.clone());
        assert_eq!(forced, Some(vec![(0.0, 1.0), (0.0, 3.0)]), "on the baseline once refueling hides");
    }

    #[test]
    fn a_band_steps_across_each_month() {
        let months = [month(&[(Family::Grid, 1.0)]), month(&[(Family::Grid, 2.0)])];
        let bands = bands(&stack(&months, Families::NONE.with(Family::Grid)), 2.0);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].path, "M0.0,120.00L500.0,120.00L500.0,0.00L1000.0,0.00L1000.0,240.00L500.0,240.00L500.0,240.00L0.0,240.00Z");
        assert_eq!(bands[0].top, "M0.0,120.00L500.0,120.00L500.0,0.00L1000.0,0.00");
    }

    #[test]
    fn seams_stop_past_ten_years() {
        assert!(seamed(36));
        assert!(seamed(120));
        assert!(!seamed(335));
    }

    #[test]
    fn ticks_land_on_round_numbers_above_the_peak() {
        assert_eq!(ticks(7.3, 4), (8.0, vec![0.0, 2.0, 4.0, 6.0, 8.0]));
        assert_eq!(ticks(14.2, 4), (15.0, vec![0.0, 5.0, 10.0, 15.0]));
        assert_eq!(ticks(0.0, 4), (1.0, vec![0.0, 1.0]));
    }

    #[test]
    fn a_pointer_finds_its_month() {
        assert_eq!(month_at(0.0, 10), Some(0));
        assert_eq!(month_at(0.55, 10), Some(5));
        assert_eq!(month_at(1.0, 10), Some(9));
        assert_eq!(month_at(1.5, 10), None);
        assert_eq!(month_at(0.5, 0), None);
    }
}
