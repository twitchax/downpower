//! The views' shared filter state, kept in the URL query so every view is a permalink (DESIGN.md
//! Decisions 23 and 24): `?from=2010-01-01&to=2020-12-31&layer=observed&families=forced,grid&events=1&table=1`.
//! A parameter at its default is left out.

use chrono::{Datelike as _, NaiveDate};

use downpower_core::Families;
use downpower_core::barcode::Layer;

/// What the barcode and the stacked area show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewState {
    /// The first and last day shown; `None` runs to the data's edge.
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub layer: Layer,
    /// The families drawn in color; the rest recede to gray.
    pub families: Families,
    /// Whether the barcode draws the event markers, off by default (Decision 35).
    pub events: bool,
    /// Whether the stacked area shows its table view beneath the chart.
    pub table: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            layer: Layer::default(),
            families: Families::ALL,
            events: false,
            table: false,
        }
    }
}

impl ViewState {
    /// The state a URL's query names, from a lookup of its parameters. A parameter that doesn't
    /// parse keeps its default, so a mangled link still opens.
    #[must_use]
    pub fn from_query(param: impl Fn(&str) -> Option<String>) -> Self {
        let date = |name: &str| param(name).and_then(|text| text.parse().ok());
        Self {
            from: date("from"),
            to: date("to"),
            layer: param("layer").and_then(|slug| Layer::from_slug(&slug)).unwrap_or_default(),
            families: param("families").map_or(Families::ALL, |param| Families::from_param(&param)),
            events: param("events").as_deref() == Some("1"),
            table: param("table").as_deref() == Some("1"),
        }
    }

    /// The query string for this state, with its `?`, or empty at the defaults.
    #[must_use]
    pub fn to_query(&self) -> String {
        let mut params = Vec::new();
        if let Some(from) = self.from {
            params.push(format!("from={from}"));
        }
        if let Some(to) = self.to {
            params.push(format!("to={to}"));
        }
        if self.layer != Layer::default() {
            params.push(format!("layer={}", self.layer.slug()));
        }
        if self.families != Families::ALL {
            params.push(format!("families={}", self.families.to_param()));
        }
        if self.events {
            params.push("events=1".to_owned());
        }
        if self.table {
            params.push("table=1".to_owned());
        }
        if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) }
    }
}

/// Year marks for an axis over `days` days from `first`: each January 1st inside the span, at its
/// fraction of the width, thinned to every 2nd, 5th or 10th year until at most `most` remain.
#[must_use]
pub fn year_ticks(first: NaiveDate, days: u32, most: usize) -> Vec<(i32, f64)> {
    if days == 0 {
        return Vec::new();
    }
    let last = first + chrono::Days::new(u64::from(days - 1));
    let years = (first.year()..=last.year()).filter_map(|year| {
        let start = NaiveDate::from_ymd_opt(year, 1, 1).filter(|start| *start >= first)?;
        #[allow(clippy::cast_precision_loss, reason = "day counts far below 2^52")]
        let fraction = (start - first).num_days() as f64 / f64::from(days);
        Some((year, fraction))
    });
    thin(years.collect(), most)
}

/// Whether an axis label at `fraction` of the width hangs left of its tick: a label in the last
/// twentieth would run past the plot's right edge, and on a phone past the screen's.
#[must_use]
pub fn hangs_left(fraction: f64) -> bool {
    fraction > 0.95
}

/// Whether a year's axis label is minor: off the five-year marks, so a phone hides it.
#[must_use]
pub const fn minor(year: i32) -> bool {
    year % 5 != 0
}

/// Every 1st, 2nd, 5th, 10th or 25th of `years`, whichever first leaves at most `most`.
#[must_use]
pub fn thin(years: Vec<(i32, f64)>, most: usize) -> Vec<(i32, f64)> {
    let step = [1, 2, 5, 10, 25]
        .into_iter()
        .find(|step| years.iter().filter(|(year, _)| year % step == 0).count() <= most.max(1))
        .unwrap_or(50);
    years.into_iter().filter(|(year, _)| year % step == 0).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use downpower_core::Family;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn labels_in_the_last_twentieth_hang_left_and_five_year_marks_stay_on_phones() {
        assert!(!hangs_left(0.5));
        assert!(!hangs_left(0.95));
        assert!(hangs_left(0.98));
        assert!(!minor(2020) && !minor(2005));
        assert!(minor(2026) && minor(2002));
    }

    fn parse(query: &[(&str, &str)]) -> ViewState {
        let map: HashMap<String, String> = query.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect();
        ViewState::from_query(|name| map.get(name).cloned())
    }

    #[test]
    fn the_default_state_has_an_empty_query() {
        assert_eq!(ViewState::default().to_query(), "");
        assert_eq!(parse(&[]), ViewState::default());
    }

    #[test]
    fn a_state_round_trips_through_its_query() {
        let state = ViewState {
            from: "2010-01-01".parse().ok(),
            to: "2020-12-31".parse().ok(),
            layer: Layer::Observed,
            families: Families::NONE.with(Family::Forced).with(Family::Grid),
            events: true,
            table: true,
        };
        let query = state.to_query();
        assert_eq!(query, "?from=2010-01-01&to=2020-12-31&layer=observed&families=grid,forced&events=1&table=1");
        let pairs: Vec<(&str, &str)> = query.trim_start_matches('?').split('&').filter_map(|pair| pair.split_once('=')).collect();
        assert_eq!(parse(&pairs), state);
    }

    #[test]
    fn uat_event_markers_ride_in_the_url_only_when_on() {
        assert!(parse(&[("events", "1")]).events);
        assert!(!parse(&[("events", "yes")]).events);
        assert!(!ViewState::default().events);
        assert_eq!(ViewState { events: true, ..ViewState::default() }.to_query(), "?events=1");
    }

    #[test]
    fn the_table_view_rides_in_the_url_only_when_shown() {
        assert!(parse(&[("table", "1")]).table);
        assert!(!parse(&[("table", "true")]).table);
        assert!(!ViewState::default().table);
        assert_eq!(ViewState { table: true, ..ViewState::default() }.to_query(), "?table=1");
    }

    #[test]
    fn a_mangled_parameter_keeps_its_default() {
        let state = parse(&[("from", "last tuesday"), ("layer", "both"), ("families", "forced,heat")]);
        assert_eq!(state.from, None);
        assert_eq!(state.layer, Layer::Attributed);
        assert_eq!(state.families, Families::NONE.with(Family::Forced));
    }

    #[test]
    fn year_ticks_thin_to_fit() {
        let first: NaiveDate = "1999-01-01".parse().unwrap_or_default();
        let days = u32::try_from(("2026-11-14".parse::<NaiveDate>().unwrap_or_default() - first).num_days() + 1).unwrap_or_default();
        let all = year_ticks(first, days, 40);
        assert_eq!(all.len(), 28);
        assert_eq!(all[0], (1999, 0.0));
        let thinned: Vec<i32> = year_ticks(first, days, 6).into_iter().map(|(year, _)| year).collect();
        assert_eq!(thinned, [2000, 2005, 2010, 2015, 2020, 2025]);
        // A span starting mid-year has no tick for that year.
        let mid: NaiveDate = "2024-07-01".parse().unwrap_or_default();
        assert_eq!(year_ticks(mid, 400, 10).iter().map(|(year, _)| *year).collect::<Vec<_>>(), [2025]);
        assert_eq!(year_ticks(mid, 0, 10), []);
    }
}
