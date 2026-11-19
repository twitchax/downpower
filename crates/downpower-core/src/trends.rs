//! The fleet's trends (PRD-0006): per month, the in-service exposure, the net capacity energy the
//! losses are weighed against and the Event Notification scrams, plus every refueling outage. A
//! rebuild collects them beside the monthly series, and the trends page turns them into rates, since
//! a fleet that changes size makes raw totals misleading across years.

use anyhow::Context as _;
use chrono::{Datelike as _, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::series::{Monthly, month_of, month_range, months_between};
use crate::{Families, Family, Res};

/// Days in an average year, for unit-years.
pub const DAYS_PER_YEAR: f64 = 365.25;

/// How a unit scrammed, by its SCRAM Code: `A/R` and `A` are automatic, `M/R` and `M` manual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScramKind {
    Automatic,
    Manual,
}

impl ScramKind {
    pub const ALL: [Self; 2] = [Self::Automatic, Self::Manual];

    /// The kind's slot in [`Month::scrams`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Manual => "Manual",
        }
    }

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Manual => "manual",
        }
    }
}

/// One month of the fleet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Month {
    /// In-service unit-days: the ledger's rows (Decision 16).
    pub unit_days: u32,
    /// Net capacity × 24 h over the rows that have a capacity, in kWh: the same rows a row's
    /// `lost_kwh` is weighed on, so a family's share is its lost kWh over this.
    pub capacity_kwh: u64,
    /// Unretracted EN scrams on catalog units, by [`ScramKind::index`].
    pub scrams: [u32; 2],
}

/// A refueling outage: a unit's run of days at 0% power, with a day at power on each side, whose
/// attributed days are mostly Refueling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outage {
    pub docket: String,
    /// The first day at 0%.
    pub start: NaiveDate,
    /// Days at 0%.
    pub days: u32,
}

/// A build's trends, as `/api/trends` serves them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trends {
    /// The first day of the first month; `None` while empty.
    pub first: Option<NaiveDate>,
    /// The last day with rows.
    pub last: Option<NaiveDate>,
    /// One entry per month from `first`.
    pub months: Vec<Month>,
    /// Every refueling outage, by start date.
    pub outages: Vec<Outage>,
}

impl Trends {
    /// Counts one in-service unit-day and its capacity energy.
    ///
    /// # Errors
    ///
    /// When `date` falls before the first month added.
    pub fn add_day(&mut self, date: NaiveDate, capacity_kw: Option<u32>) -> Res<()> {
        let first = *self.first.get_or_insert(month_of(date));
        let index = usize::try_from(months_between(first, date)).with_context(|| format!("{date} is before the trends' first month, {first}"))?;
        if self.months.len() <= index {
            self.months.resize(index + 1, Month::default());
        }
        let month = &mut self.months[index];
        month.unit_days += 1;
        month.capacity_kwh += capacity_kw.map_or(0, |kw| u64::from(kw) * 24);
        self.last = self.last.max(Some(date));
        Ok(())
    }

    /// Counts a scram in its month, and returns whether the months cover it: a scram outside them
    /// has no exposure to weigh it against, so it counts toward nothing.
    pub fn add_scram(&mut self, date: NaiveDate, kind: ScramKind) -> bool {
        let Some(first) = self.first else { return false };
        let Some(month) = usize::try_from(months_between(first, date)).ok().and_then(|index| self.months.get_mut(index)) else {
            return false;
        };
        month.scrams[kind.index()] += 1;
        true
    }

    /// The months whose first day falls from `from`'s month to `to`'s, as indexes.
    #[must_use]
    pub fn range(&self, from: Option<NaiveDate>, to: Option<NaiveDate>) -> std::ops::Range<usize> {
        month_range(self.first, self.months.len(), from, to)
    }

    /// Whether the days shown, the range clipped to the data, leave out part of `year`.
    fn partial(&self, year: i32, from: Option<NaiveDate>, to: Option<NaiveDate>) -> bool {
        let (Some(january), Some(december)) = (NaiveDate::from_ymd_opt(year, 1, 1), NaiveDate::from_ymd_opt(year, 12, 31)) else {
            return true;
        };
        let start = from.max(self.first);
        let end = match (to, self.last) {
            (Some(to), Some(last)) => Some(to.min(last)),
            (to, last) => to.or(last),
        };
        start.is_none_or(|start| start > january) || end.is_none_or(|end| end < december)
    }

    /// The range's months, each with its first day.
    fn dated(&self, from: Option<NaiveDate>, to: Option<NaiveDate>) -> impl Iterator<Item = (NaiveDate, &Month)> {
        let first = self.first;
        self.range(from, to).filter_map(move |index| {
            let date = first?.checked_add_months(chrono::Months::new(u32::try_from(index).ok()?))?;
            Some((date, self.months.get(index)?))
        })
    }
}

/// One year of the range, summed over the months of it the range holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Year {
    pub year: i32,
    pub unit_days: u32,
    pub capacity_kwh: u64,
    /// Reported power reduction by [`Family::index`], in the layer the monthly series is from.
    pub lost_kwh: [u64; 8],
    pub scrams: [u32; 2],
    /// The days shown leave out part of the year: the range starts or ends inside it, or the data
    /// does.
    pub partial: bool,
}

/// `numerator` over `denominator` as a percentage, or none over nothing.
#[allow(clippy::cast_precision_loss, reason = "kWh totals far below 2^52")]
fn percent(numerator: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64 * 100.0)
}

impl Year {
    #[must_use]
    pub fn unit_years(&self) -> f64 {
        f64::from(self.unit_days) / DAYS_PER_YEAR
    }

    /// `family`'s reported power reduction as a percentage of net capacity, none without capacity.
    #[must_use]
    pub fn share(&self, family: Family) -> Option<f64> {
        percent(self.lost_kwh[usize::from(family.index())], self.capacity_kwh)
    }

    /// The visible families' shares summed.
    #[must_use]
    pub fn total_share(&self, visible: Families) -> Option<f64> {
        percent(visible.iter().map(|family| self.lost_kwh[usize::from(family.index())]).sum(), self.capacity_kwh)
    }

    /// Scrams of `kind` per 100 unit-years, none without exposure.
    #[must_use]
    pub fn scram_rate(&self, kind: ScramKind) -> Option<f64> {
        (self.unit_days > 0).then(|| f64::from(self.scrams[kind.index()]) / self.unit_years() * 100.0)
    }
}

/// The years from `from`'s through `to`'s, each summing the months the range holds, with the
/// reported power reduction from `monthly` (a layer's series from the same build).
#[must_use]
pub fn years(trends: &Trends, monthly: &Monthly, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Vec<Year> {
    let mut years: Vec<Year> = Vec::new();
    for (date, month) in trends.dated(from, to) {
        if years.last().is_none_or(|last| last.year != date.year()) {
            years.push(Year {
                year: date.year(),
                unit_days: 0,
                capacity_kwh: 0,
                lost_kwh: [0; 8],
                scrams: [0; 2],
                partial: trends.partial(date.year(), from, to),
            });
        }
        let Some(year) = years.last_mut() else { continue };
        year.unit_days += month.unit_days;
        year.capacity_kwh += month.capacity_kwh;
        for (total, scrams) in year.scrams.iter_mut().zip(month.scrams) {
            *total += scrams;
        }
        for (total, lost) in year.lost_kwh.iter_mut().zip(lost_in(monthly, date)) {
            *total += lost;
        }
    }
    years
}

/// `monthly`'s reported power reduction in the month starting `date`, zeros where it has none.
fn lost_in(monthly: &Monthly, date: NaiveDate) -> [u64; 8] {
    monthly
        .first
        .and_then(|first| usize::try_from(months_between(first, date)).ok())
        .and_then(|index| monthly.lost_kwh.get(index))
        .copied()
        .unwrap_or([0; 8])
}

/// One calendar month across the range's years.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Season {
    /// 1 for January through 12.
    pub month: u32,
    /// How many of the range's months are this calendar month.
    pub months: u32,
    pub capacity_kwh: u64,
    pub lost_kwh: [u64; 8],
}

impl Season {
    /// `family`'s reported power reduction as a percentage of net capacity, none without capacity.
    #[must_use]
    pub fn share(&self, family: Family) -> Option<f64> {
        percent(self.lost_kwh[usize::from(family.index())], self.capacity_kwh)
    }

    /// The visible families' shares summed.
    #[must_use]
    pub fn total_share(&self, visible: Families) -> Option<f64> {
        percent(visible.iter().map(|family| self.lost_kwh[usize::from(family.index())]).sum(), self.capacity_kwh)
    }
}

/// The twelve calendar months, January first, each summing the range's months that fall in it.
#[must_use]
pub fn seasons(trends: &Trends, monthly: &Monthly, from: Option<NaiveDate>, to: Option<NaiveDate>) -> [Season; 12] {
    let mut seasons: [Season; 12] = std::array::from_fn(|index| Season {
        month: u32::try_from(index + 1).unwrap_or(0),
        ..Season::default()
    });
    for (date, month) in trends.dated(from, to) {
        let Some(season) = seasons.get_mut(date.month0() as usize) else { continue };
        season.months += 1;
        season.capacity_kwh += month.capacity_kwh;
        for (total, lost) in season.lost_kwh.iter_mut().zip(lost_in(monthly, date)) {
            *total += lost;
        }
    }
    seasons
}

/// The refueling outages that started in one year of the range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutageYear {
    pub year: i32,
    pub count: u32,
    /// Days at 0%: the median, and the first and third quartiles.
    pub median: f64,
    pub quartiles: (f64, f64),
    pub shortest: u32,
    pub longest: u32,
    pub partial: bool,
}

/// The `p` quantile of `sorted` (ascending, not empty), interpolating between closest ranks.
fn quantile(sorted: &[u32], p: f64) -> f64 {
    let Some(&last) = sorted.last() else { return 0.0 };
    #[allow(clippy::cast_precision_loss, reason = "a few hundred outages")]
    let rank = (sorted.len() - 1) as f64 * p.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a rank inside the slice")]
    let below = rank.floor() as usize;
    let lower = f64::from(sorted.get(below).copied().unwrap_or(last));
    let upper = f64::from(sorted.get(below + 1).copied().unwrap_or(last));
    lower + (rank - rank.floor()) * (upper - lower)
}

/// Per year of the range, the refueling outages that started inside the range.
#[must_use]
pub fn outage_years(trends: &Trends, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Vec<OutageYear> {
    let inside = |start: NaiveDate| from.is_none_or(|from| from <= start) && to.is_none_or(|to| start <= to);
    let mut by_year: std::collections::BTreeMap<i32, Vec<u32>> = std::collections::BTreeMap::new();
    for outage in trends.outages.iter().filter(|outage| inside(outage.start)) {
        by_year.entry(outage.start.year()).or_default().push(outage.days);
    }
    by_year
        .into_iter()
        .map(|(year, mut days)| {
            days.sort_unstable();
            OutageYear {
                year,
                count: u32::try_from(days.len()).unwrap_or(u32::MAX),
                median: quantile(&days, 0.5),
                quartiles: (quantile(&days, 0.25), quantile(&days, 0.75)),
                shortest: days.first().copied().unwrap_or(0),
                longest: days.last().copied().unwrap_or(0),
                partial: trends.partial(year, from, to),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    /// Two units over 2023-11 to 2024-02: 2023 has two months, so it is partial; the data ends on
    /// 2024-02-10, so 2024 is too.
    fn fixture() -> Res<(Trends, Monthly)> {
        let mut trends = Trends::default();
        let mut monthly = Monthly::default();
        for day in date("2023-11-01").iter_days().take_while(|day| *day <= date("2024-02-10")) {
            trends.add_day(day, Some(1_000))?;
            trends.add_day(day, None)?;
            monthly.add(day, Family::Refueling, if day.month() == 1 { 2_400 } else { 0 })?;
            monthly.add(day, Family::Environmental, if day.month() == 2 { 1_200 } else { 0 })?;
        }
        assert!(trends.add_scram(date("2024-01-20"), ScramKind::Automatic));
        assert!(trends.add_scram(date("2024-02-03"), ScramKind::Manual));
        assert!(!trends.add_scram(date("2023-10-31"), ScramKind::Manual), "before the months");
        assert!(!trends.add_scram(date("2024-03-01"), ScramKind::Manual), "after them");
        trends.outages = vec![
            Outage {
                docket: "05000001".to_owned(),
                start: date("2023-11-05"),
                days: 30,
            },
            Outage {
                docket: "05000002".to_owned(),
                start: date("2024-01-02"),
                days: 20,
            },
            Outage {
                docket: "05000001".to_owned(),
                start: date("2024-01-09"),
                days: 40,
            },
            Outage {
                docket: "05000002".to_owned(),
                start: date("2024-02-01"),
                days: 25,
            },
        ];
        Ok((trends, monthly))
    }

    #[test]
    fn uat_trends_aggregate_months_count_days_capacity_and_scrams() -> Res<()> {
        let (mut trends, _) = fixture()?;
        assert_eq!(trends.first, Some(date("2023-11-01")));
        assert_eq!(trends.last, Some(date("2024-02-10")));
        assert_eq!(trends.months.len(), 4);
        assert_eq!(
            trends.months[2],
            Month {
                unit_days: 62,
                capacity_kwh: 31 * 24_000,
                scrams: [1, 0],
            }
        );
        assert_eq!(trends.months[3].scrams, [0, 1]);
        assert!(trends.add_day(date("2023-10-31"), None).is_err());
        Ok(())
    }

    #[test]
    fn uat_trends_aggregate_years_weigh_losses_against_capacity() -> Res<()> {
        let (trends, monthly) = fixture()?;
        let years = years(&trends, &monthly, None, None);
        assert_eq!(years.iter().map(|year| (year.year, year.partial)).collect::<Vec<_>>(), [(2023, true), (2024, true)]);
        let year = years[1];
        assert_eq!(year.unit_days, 2 * (31 + 10));
        assert_eq!(year.capacity_kwh, (31 + 10) * 24_000);
        // 31 days of 2,400 kWh Refueling against 41 days of 24,000 kWh capacity.
        assert!(close(year.share(Family::Refueling), 31.0 * 2_400.0 / (41.0 * 24_000.0) * 100.0));
        assert!(close(year.share(Family::Environmental), 10.0 * 1_200.0 / (41.0 * 24_000.0) * 100.0));
        let visible = Families::ALL.toggled(Family::Environmental);
        assert!(close(year.total_share(visible), year.share(Family::Refueling).unwrap_or_default()));
        assert!(close(year.scram_rate(ScramKind::Automatic), 1.0 / (82.0 / DAYS_PER_YEAR) * 100.0));
        assert_eq!(years[0].share(Family::Refueling), Some(0.0));
        assert_eq!(Year { capacity_kwh: 0, unit_days: 0, ..year }.share(Family::Refueling), None);
        assert_eq!(Year { capacity_kwh: 0, unit_days: 0, ..year }.scram_rate(ScramKind::Manual), None);
        Ok(())
    }

    #[test]
    fn uat_trends_aggregate_the_range_picks_the_months_and_flags_partial_years() -> Res<()> {
        let (trends, monthly) = fixture()?;
        let ranged = years(&trends, &monthly, Some(date("2024-01-01")), Some(date("2024-01-31")));
        assert_eq!(ranged.len(), 1);
        assert_eq!((ranged[0].year, ranged[0].unit_days, ranged[0].partial), (2024, 62, true));

        // A full year inside the data and the range isn't partial.
        let mut whole = Trends::default();
        for day in date("2020-01-01").iter_days().take_while(|day| *day <= date("2021-03-01")) {
            whole.add_day(day, Some(1))?;
        }
        let years = years(&whole, &Monthly::default(), None, None);
        assert_eq!(years.iter().map(|year| (year.year, year.partial)).collect::<Vec<_>>(), [(2020, false), (2021, true)]);
        assert_eq!(years[0].share(Family::Refueling), Some(0.0), "no monthly series: nothing lost");
        Ok(())
    }

    #[test]
    fn uat_trends_aggregate_seasons_sum_each_calendar_month() -> Res<()> {
        let (trends, monthly) = fixture()?;
        let seasons = seasons(&trends, &monthly, None, None);
        assert_eq!(seasons.map(|season| season.month), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(seasons.map(|season| season.months), [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1]);
        assert!(close(seasons[0].share(Family::Refueling), 2_400.0 / 24_000.0 * 100.0));
        assert!(close(seasons[1].share(Family::Environmental), 1_200.0 / 24_000.0 * 100.0));
        assert_eq!(seasons[2].share(Family::Refueling), None);
        let january = seasons_in(&trends, &monthly, "2024-01-01", "2024-01-31");
        assert_eq!(january.map(|season| season.months), [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        Ok(())
    }

    fn seasons_in(trends: &Trends, monthly: &Monthly, from: &str, to: &str) -> [Season; 12] {
        seasons(trends, monthly, Some(date(from)), Some(date(to)))
    }

    #[test]
    fn uat_trends_aggregate_outage_years_take_medians_and_quartiles() -> Res<()> {
        let (trends, _) = fixture()?;
        let years = outage_years(&trends, None, None);
        assert_eq!(years.len(), 2);
        assert_eq!((years[0].year, years[0].count, years[0].median, years[0].partial), (2023, 1, 30.0, true));
        let year = years[1];
        assert_eq!((year.year, year.count, year.shortest, year.longest), (2024, 3, 20, 40));
        // 20, 25, 40: the median is 25, the quartiles interpolate halfway between neighbors.
        assert_eq!((year.median, year.quartiles), (25.0, (22.5, 32.5)));
        let ranged = outage_years(&trends, Some(date("2024-01-05")), None);
        assert_eq!(ranged.iter().map(|year| (year.year, year.count)).collect::<Vec<_>>(), [(2024, 2)]);
        Ok(())
    }

    #[test]
    fn quantiles_interpolate_between_closest_ranks() {
        assert!((quantile(&[7], 0.25) - 7.0).abs() < 1e-9);
        assert!((quantile(&[10, 20, 30, 40], 0.5) - 25.0).abs() < 1e-9);
        assert!((quantile(&[10, 20, 30, 40], 0.75) - 32.5).abs() < 1e-9);
    }

    #[test]
    fn the_json_shape_is_months_and_outages() -> Res<()> {
        let mut trends = Trends::default();
        trends.add_day(date("2024-01-15"), Some(2))?;
        let json = serde_json::to_string(&trends)?;
        assert_eq!(
            json,
            r#"{"first":"2024-01-01","last":"2024-01-15","months":[{"unit_days":1,"capacity_kwh":48,"scrams":[0,0]}],"outages":[]}"#
        );
        assert_eq!(serde_json::from_str::<Trends>(&json)?, trends);
        Ok(())
    }
}
