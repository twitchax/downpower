//! Lost energy by family and month: the stacked area's data (DESIGN.md Decisions 22 and 24). The
//! server sums it during a rebuild, so the view never reads the million rows behind it.

use anyhow::{Context as _, ensure};
use chrono::{Datelike as _, Months, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::{Family, Res};

/// The first of `date`'s month.
#[must_use]
pub fn month_of(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

/// Whole months from `from`'s month to `to`'s.
pub(crate) fn months_between(from: NaiveDate, to: NaiveDate) -> i64 {
    i64::from(to.year() - from.year()) * 12 + i64::from(to.month()) - i64::from(from.month())
}

/// Of `count` months from `first`, those whose first day falls from `from`'s month to `to`'s, as
/// indexes: every month when the range is inverted.
#[must_use]
pub fn month_range(first: Option<NaiveDate>, count: usize, from: Option<NaiveDate>, to: Option<NaiveDate>) -> std::ops::Range<usize> {
    let Some(first) = first else { return 0..0 };
    let clamp = |date: NaiveDate| usize::try_from(months_between(first, date).max(0)).unwrap_or(0).min(count);
    let start = from.map_or(0, clamp);
    let end = to.map_or(count, |to| clamp(to).saturating_add(1).min(count));
    if end <= start { 0..count } else { start..end }
}

/// Reported power reduction in kWh, per month and family (Decision 22's label: never "heat loss").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Monthly {
    /// The first day of the first month; `None` while empty.
    pub first: Option<NaiveDate>,
    /// One entry per month from `first`, each indexed by [`Family::index`].
    pub lost_kwh: Vec<[u64; 8]>,
}

impl Monthly {
    /// Adds `kwh` lost on `date` to `family`.
    ///
    /// # Errors
    ///
    /// When `date` falls before the first month added.
    pub fn add(&mut self, date: NaiveDate, family: Family, kwh: u64) -> Res<()> {
        let first = *self.first.get_or_insert(month_of(date));
        let index = usize::try_from(months_between(first, date)).with_context(|| format!("{date} is before the series' first month, {first}"))?;
        if self.lost_kwh.len() <= index {
            self.lost_kwh.resize(index + 1, [0; 8]);
        }
        self.lost_kwh[index][usize::from(family.index())] += kwh;
        Ok(())
    }

    /// The first day of month `index`.
    #[must_use]
    pub fn month(&self, index: usize) -> Option<NaiveDate> {
        self.first?.checked_add_months(Months::new(u32::try_from(index).ok()?))
    }

    /// The months whose first day falls from `from`'s month to `to`'s, as indexes.
    #[must_use]
    pub fn range(&self, from: Option<NaiveDate>, to: Option<NaiveDate>) -> std::ops::Range<usize> {
        month_range(self.first, self.lost_kwh.len(), from, to)
    }

    /// Checks the shape a client relies on.
    ///
    /// # Errors
    ///
    /// When there are months but no first month.
    pub fn check(&self) -> Res<()> {
        ensure!(self.first.is_some() || self.lost_kwh.is_empty(), "months without a first month");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    #[test]
    fn days_sum_into_their_month_and_family() -> Res<()> {
        let mut monthly = Monthly::default();
        monthly.add(date("2024-01-15"), Family::Refueling, 100)?;
        monthly.add(date("2024-01-31"), Family::Refueling, 50)?;
        monthly.add(date("2024-03-01"), Family::Forced, 7)?;
        assert_eq!(monthly.first, Some(date("2024-01-01")));
        assert_eq!(monthly.lost_kwh.len(), 3);
        assert_eq!(monthly.lost_kwh[0][usize::from(Family::Refueling.index())], 150);
        assert_eq!(monthly.lost_kwh[1], [0; 8]);
        assert_eq!(monthly.lost_kwh[2][usize::from(Family::Forced.index())], 7);
        assert_eq!(monthly.month(2), Some(date("2024-03-01")));
        assert!(monthly.add(date("2023-12-31"), Family::Grid, 1).is_err());
        monthly.check()
    }

    #[test]
    fn a_range_clamps_to_the_months_held() -> Res<()> {
        let mut monthly = Monthly::default();
        for day in ["2024-01-01", "2024-06-01", "2024-12-01"] {
            monthly.add(date(day), Family::Grid, 1)?;
        }
        assert_eq!(monthly.range(None, None), 0..12);
        assert_eq!(monthly.range(Some(date("2024-03-15")), Some(date("2024-05-02"))), 2..5);
        assert_eq!(monthly.range(Some(date("2020-01-01")), Some(date("2030-01-01"))), 0..12);
        assert_eq!(monthly.range(Some(date("2024-05-01")), Some(date("2024-02-01"))), 0..12, "inverted: everything");
        assert_eq!(Monthly::default().range(None, None), 0..0);
        Ok(())
    }

    #[test]
    fn the_json_shape_is_first_and_a_row_per_month() -> Res<()> {
        let mut monthly = Monthly::default();
        monthly.add(date("2024-01-15"), Family::FuelCycle, 9)?;
        let json = serde_json::to_string(&monthly)?;
        assert_eq!(json, r#"{"first":"2024-01-01","lost_kwh":[[0,9,0,0,0,0,0,0]]}"#);
        assert_eq!(serde_json::from_str::<Monthly>(&json)?, monthly);
        Ok(())
    }
}
