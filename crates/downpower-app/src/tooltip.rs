//! Where a hover readout sits beside the pointer, for the barcode and the stacked area alike.

/// The space between the pointer and the readout's near edge.
const GAP: &str = "12px";

/// A readout's `left` and `right` for a pointer `at` (a CSS length from the plot's left edge): its
/// left edge just right of the pointer, or with `flip` (near the plot's right edge) its right edge
/// just left of it. The flip anchors `right` because a percentage in `left` is of the plot, not
/// the readout: `left: calc(at - 100%)` put a flipped readout a plot-width to the left, off the plot.
pub fn beside(at: &str, flip: bool) -> (Option<String>, Option<String>) {
    if flip {
        (None, Some(format!("calc(100% - {at} + {GAP})")))
    } else {
        (Some(format!("calc({at} + {GAP})")), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a browser resolves one of `beside`'s lengths to in a plot `plot` pixels wide: a sum of
    /// `px` and `%` (of the plot) terms.
    fn resolve(length: &str, plot: f64) -> Option<f64> {
        let length = length.strip_prefix("calc(")?.strip_suffix(')')?;
        let mut sign = 1.0;
        let mut total = 0.0;
        for token in length.split_whitespace() {
            match token {
                "+" => sign = 1.0,
                "-" => sign = -1.0,
                _ => match token.strip_suffix('%') {
                    Some(percent) => total += sign * percent.parse::<f64>().ok()? / 100.0 * plot,
                    None => total += sign * token.strip_suffix("px")?.parse::<f64>().ok()?,
                },
            }
        }
        Some(total)
    }

    /// The readout's left and right edges, from the plot's left edge, for a readout `width` wide.
    fn edges(anchors: (Option<String>, Option<String>), plot: f64, width: f64) -> Option<(f64, f64)> {
        match anchors {
            (Some(left), None) => resolve(&left, plot).map(|left| (left, left + width)),
            (None, Some(right)) => resolve(&right, plot).map(|right| (plot - right - width, plot - right)),
            _ => None,
        }
    }

    fn at(edges: Option<(f64, f64)>, expected: (f64, f64)) -> bool {
        edges.is_some_and(|(left, right)| (left - expected.0).abs() < 1e-6 && (right - expected.1).abs() < 1e-6)
    }

    #[test]
    fn a_readout_sits_just_right_of_the_pointer_and_flips_to_end_just_left_of_it() {
        // The live barcode at 1216px: the pointer at 1094px put the old flip at -134px.
        assert!(at(edges(beside("1094px", true), 1216.0, 320.0), (762.0, 1082.0)));
        assert!(at(edges(beside("243px", false), 1216.0, 320.0), (255.0, 575.0)));
        // The stacked area places by fraction, and the flipped readout ends 12px short of the
        // pointer whatever its own width.
        assert!(at(edges(beside("75%", true), 1216.0, 180.0), (720.0, 900.0)));
        assert!(at(edges(beside("75%", true), 1216.0, 242.0), (658.0, 900.0)));
        assert!(at(edges(beside("20%", false), 1000.0, 242.0), (212.0, 454.0)));
    }
}
