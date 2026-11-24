//! Scram triggers (DESIGN.md Decisions 40 to 43, PRD-0007): what started each Event Notification
//! scram, as the ordered phrase rules in `catalog/triggers.toml` tag it, and what the scram cost,
//! its episode. Each dimension has a value for "none named", so a scram filed with its cause under
//! investigation shows rather than dropping out.

use chrono::{Datelike as _, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::series::{month_range, months_between};
use crate::trends::{DAYS_PER_YEAR, ScramKind, Trends};

/// The plant system the text says started the scram: the most upstream cause it names, and when
/// it names only the protective signal, the signal's own system (Decision 41).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum System {
    /// Feed and condensate pumps, feed regulating valves, heaters and drains, level control.
    Feedwater,
    /// The main turbine and its controls, the main generator and its protection.
    TurbineGenerator,
    /// The plant's own power distribution: transformers, buses, breakers, inverters.
    Electrical,
    /// The switchyard, transmission lines and the grid.
    OffsitePower,
    /// Condenser vacuum, circulating and service water, the intake.
    CoolingWater,
    /// Control rods, coolant and recirculation pumps, the pressurizer, relief valves, main steam
    /// isolation.
    ReactorCoolant,
    /// A failed sensor, a card, a spurious protection signal.
    Instruments,
    /// The text names no system: the cause is under investigation, or no system started it.
    NotNamed,
}

impl System {
    pub const ALL: [Self; 8] = [
        Self::Feedwater,
        Self::TurbineGenerator,
        Self::Electrical,
        Self::OffsitePower,
        Self::CoolingWater,
        Self::ReactorCoolant,
        Self::Instruments,
        Self::NotNamed,
    ];

    /// The system's slot in a per-system array, in [`System::ALL`]'s order.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Feedwater => "Feedwater and condensate",
            Self::TurbineGenerator => "Turbine and generator",
            Self::Electrical => "Plant electrical",
            Self::OffsitePower => "Offsite power",
            Self::CoolingWater => "Cooling water",
            Self::ReactorCoolant => "Reactor and coolant system",
            Self::Instruments => "Instruments",
            Self::NotNamed => "Not named",
        }
    }

    /// The value as `catalog/triggers.toml` and the URL write it.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Feedwater => "feedwater",
            Self::TurbineGenerator => "turbine_generator",
            Self::Electrical => "electrical",
            Self::OffsitePower => "offsite_power",
            Self::CoolingWater => "cooling_water",
            Self::ReactorCoolant => "reactor_coolant",
            Self::Instruments => "instruments",
            Self::NotNamed => "not_named",
        }
    }

    /// The system a slug names.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|system| system.slug() == slug)
    }
}

/// What was going on that the text ties to the scram, the first that applies in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Circumstance {
    Weather,
    Testing,
    Maintenance,
    /// An operator, technician or procedure error, outside testing and maintenance.
    HumanError,
    #[serde(rename = "none")]
    Unnamed,
}

impl Circumstance {
    pub const ALL: [Self; 5] = [Self::Weather, Self::Testing, Self::Maintenance, Self::HumanError, Self::Unnamed];

    /// The circumstance's slot in a per-circumstance array, in [`Circumstance::ALL`]'s order.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Weather => "Weather",
            Self::Testing => "Testing",
            Self::Maintenance => "Maintenance",
            Self::HumanError => "Human error",
            Self::Unnamed => "None named",
        }
    }

    /// The value as `catalog/triggers.toml` writes it.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Weather => "weather",
            Self::Testing => "testing",
            Self::Maintenance => "maintenance",
            Self::HumanError => "human_error",
            Self::Unnamed => "none",
        }
    }

    /// The circumstance a slug names.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|circumstance| circumstance.slug() == slug)
    }
}

/// A scram's tags, one per dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tags {
    pub system: System,
    pub circumstance: Circumstance,
}

impl Default for Tags {
    fn default() -> Self {
        Self {
            system: System::NotNamed,
            circumstance: Circumstance::Unnamed,
        }
    }
}

/// How an episode ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// The unit came back within [`DROP`] points of the power it scrammed from.
    Returned,
    /// A day attributed to Refueling: the outage after it is the outage's, not the scram's.
    Refueling,
    /// The unit's next scram from at least [`DROP`]% power, which starts its own episode.
    NextScram,
    /// The rows stopped first: a day missing, the unit out of service, or the end of the data.
    Open,
}

/// How many points below the power it scrammed from a unit must report to be down from the scram:
/// the scram check's drop (Decision 33). An episode runs while the unit is this far down.
pub const DROP: u8 = 10;

/// What a scram cost (Decision 42): the unit's days from the drop until it is back within
/// [`DROP`] points of its pre-scram power, whatever cause each day is attributed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    /// The first day down: the day of the event or the next.
    pub start: NaiveDate,
    /// Days down.
    pub days: u32,
    /// (pre-scram power − power)% × net capacity × 24 h over those days, in kWh; a day without a
    /// capacity adds nothing.
    pub lost_kwh: u64,
    pub end: End,
}

/// One unit's scram: an unretracted Event Notification giving a catalog unit a SCRAM Code other
/// than `N`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scram {
    pub number: u32,
    pub docket: String,
    /// The day the event occurred.
    pub date: NaiveDate,
    pub kind: ScramKind,
    /// The unit's power before the scram, from the event's unit table.
    pub initial_power: u8,
    #[serde(flatten)]
    pub tags: Tags,
    /// None when the scram was from under [`DROP`]% power or the reports don't show the drop.
    pub episode: Option<Episode>,
}

/// One unit's in-service days by month, for the unit table's rates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitExposure {
    pub docket: String,
    pub name: String,
    /// In-service days per month from [`Series::first`]: the ledger's rows.
    pub days: Vec<u8>,
}

/// A build's scram series, as `/api/scrams` serves it. The fleet's exposure and capacity come
/// from the same build's trends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    /// The first day of the first month; `None` while empty.
    pub first: Option<NaiveDate>,
    /// The last day with rows.
    pub last: Option<NaiveDate>,
    /// Every unit's scram, by date, event number and docket.
    pub scrams: Vec<Scram>,
    /// Every catalog unit with rows, by docket.
    pub units: Vec<UnitExposure>,
}

/// One year of the range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Year {
    pub year: i32,
    pub unit_days: u32,
    pub capacity_kwh: u64,
    /// Scrams by [`System::index`].
    pub scrams: [u32; 8],
    /// Scrams by [`Circumstance::index`].
    pub circumstances: [u32; 5],
    /// Episode energy by [`System::index`], counted in the scram's month.
    pub lost_kwh: [u64; 8],
    /// The days shown leave out part of the year.
    pub partial: bool,
}

impl Year {
    /// Every scram in the year.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.scrams.iter().sum()
    }

    #[must_use]
    pub fn unit_years(&self) -> f64 {
        f64::from(self.unit_days) / DAYS_PER_YEAR
    }

    /// `count` per 100 unit-years, none without exposure.
    #[must_use]
    pub fn rate(&self, count: u32) -> Option<f64> {
        (self.unit_days > 0).then(|| f64::from(count) / self.unit_years() * 100.0)
    }

    /// `system`'s episode energy as a percentage of net capacity energy, none without capacity.
    #[must_use]
    pub fn share(&self, system: System) -> Option<f64> {
        percent(self.lost_kwh[system.index()], self.capacity_kwh)
    }
}

/// `numerator` over `denominator` as a percentage, or none over nothing.
#[allow(clippy::cast_precision_loss, reason = "kWh totals far below 2^52")]
fn percent(numerator: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64 * 100.0)
}

/// The years from `from`'s month through `to`'s, with the trends' exposure and capacity. A scram
/// counts in its month when the range holds it, and one outside the trends' months counts toward
/// nothing, as the trends' own scram counts work.
#[must_use]
pub fn years(series: &Series, trends: &Trends, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Vec<Year> {
    let mut years: Vec<Year> = Vec::new();
    for (date, month) in trends.dated(from, to) {
        if years.last().is_none_or(|last| last.year != date.year()) {
            years.push(Year {
                year: date.year(),
                unit_days: 0,
                capacity_kwh: 0,
                scrams: [0; 8],
                circumstances: [0; 5],
                lost_kwh: [0; 8],
                partial: trends.partial(date.year(), from, to),
            });
        }
        if let Some(year) = years.last_mut() {
            year.unit_days += month.unit_days;
            year.capacity_kwh += month.capacity_kwh;
        }
    }
    let range = trends.range(from, to);
    for scram in &series.scrams {
        if !month_index(trends.first, scram.date).is_some_and(|index| range.contains(&index)) {
            continue;
        }
        let Some(year) = years.iter_mut().find(|year| year.year == scram.date.year()) else { continue };
        year.scrams[scram.tags.system.index()] += 1;
        year.circumstances[scram.tags.circumstance.index()] += 1;
        year.lost_kwh[scram.tags.system.index()] += scram.episode.map_or(0, |episode| episode.lost_kwh);
    }
    years
}

/// `date`'s month as an index from `first`'s, none before it.
fn month_index(first: Option<NaiveDate>, date: NaiveDate) -> Option<usize> {
    usize::try_from(months_between(first?, date)).ok()
}

/// One unit's row of the unit table.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitRow {
    pub docket: String,
    pub name: String,
    pub unit_days: u32,
    pub scrams: u32,
    /// Scrams by [`System::index`].
    pub systems: [u32; 8],
    /// The median episode, in days; none without one.
    pub median_days: Option<f64>,
    pub lost_kwh: u64,
    /// The named system with the most scrams, the first in [`System::ALL`] on a tie.
    pub leading: Option<System>,
}

/// The unit table over the range's months, by docket.
#[must_use]
pub fn units(series: &Series, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Vec<UnitRow> {
    let count = match (series.first, series.last) {
        (Some(first), Some(last)) => usize::try_from(months_between(first, last) + 1).unwrap_or(0),
        _ => 0,
    };
    let range = month_range(series.first, count, from, to);
    let mut rows: Vec<UnitRow> = series
        .units
        .iter()
        .map(|unit| UnitRow {
            docket: unit.docket.clone(),
            name: unit.name.clone(),
            unit_days: unit.days.get(range.clone()).unwrap_or_default().iter().map(|days| u32::from(*days)).sum(),
            scrams: 0,
            systems: [0; 8],
            median_days: None,
            lost_kwh: 0,
            leading: None,
        })
        .collect();
    let mut episodes: Vec<Vec<u32>> = vec![Vec::new(); rows.len()];
    for scram in &series.scrams {
        if !month_index(series.first, scram.date).is_some_and(|index| range.contains(&index)) {
            continue;
        }
        let Some(index) = rows.iter().position(|row| row.docket == scram.docket) else { continue };
        let row = &mut rows[index];
        row.scrams += 1;
        row.systems[scram.tags.system.index()] += 1;
        if let Some(episode) = scram.episode {
            row.lost_kwh += episode.lost_kwh;
            episodes[index].push(episode.days);
        }
    }
    for (row, mut days) in rows.iter_mut().zip(episodes) {
        days.sort_unstable();
        row.median_days = median(&days);
        row.leading = System::ALL
            .into_iter()
            .filter(|system| *system != System::NotNamed && row.systems[system.index()] > 0)
            .fold(None, |best: Option<System>, system| match best {
                Some(best) if row.systems[best.index()] >= row.systems[system.index()] => Some(best),
                _ => Some(system),
            });
    }
    rows
}

/// The median of sorted `values`, none when empty.
fn median(values: &[u32]) -> Option<f64> {
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        length if length % 2 == 1 => values.get(middle).map(|value| f64::from(*value)),
        _ => Some(f64::midpoint(f64::from(values[middle - 1]), f64::from(values[middle]))),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::trends::Month;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    fn month(unit_days: u32) -> Month {
        Month {
            unit_days,
            capacity_kwh: u64::from(unit_days) * 24_000,
            scrams: [0; 2],
        }
    }

    /// Two units from 2023-12 through 2024-02-29.
    fn trends() -> Trends {
        Trends {
            first: Some(day("2023-12-01")),
            last: Some(day("2024-02-29")),
            months: vec![month(62), month(62), month(58)],
            outages: Vec::new(),
        }
    }

    fn scram(number: u32, docket: &str, date: &str, system: System, circumstance: Circumstance, episode: Option<(u32, u64)>) -> Scram {
        Scram {
            number,
            docket: docket.to_owned(),
            date: day(date),
            kind: ScramKind::Automatic,
            initial_power: 100,
            tags: Tags { system, circumstance },
            episode: episode.map(|(days, lost_kwh)| Episode {
                start: day(date),
                days,
                lost_kwh,
                end: End::Returned,
            }),
        }
    }

    fn series() -> Series {
        Series {
            first: Some(day("2023-12-01")),
            last: Some(day("2024-02-29")),
            scrams: vec![
                scram(1, "X", "2023-12-10", System::Feedwater, Circumstance::Testing, Some((2, 48_000))),
                scram(2, "X", "2024-01-05", System::NotNamed, Circumstance::Unnamed, None),
                scram(3, "Y", "2024-02-20", System::Feedwater, Circumstance::Unnamed, Some((4, 24_000))),
                // After the last month: no exposure to weigh it against.
                scram(4, "Y", "2024-03-01", System::Electrical, Circumstance::Unnamed, Some((1, 1_000))),
            ],
            units: vec![
                UnitExposure {
                    docket: "X".to_owned(),
                    name: "Unit X".to_owned(),
                    days: vec![31, 31, 29],
                },
                UnitExposure {
                    docket: "Y".to_owned(),
                    name: "Unit Y".to_owned(),
                    days: vec![31, 31, 29],
                },
            ],
        }
    }

    fn by_system(pairs: &[(System, u32)]) -> [u32; 8] {
        let mut counts = [0; 8];
        for &(system, count) in pairs {
            counts[system.index()] = count;
        }
        counts
    }

    #[test]
    fn uat_scrams_aggregate_years_count_by_system_and_circumstance_with_the_trends_exposure() {
        let years = years(&series(), &trends(), None, None);
        assert_eq!(years.len(), 2);
        let (december, winter) = (years[0], years[1]);
        assert_eq!((december.year, december.unit_days, december.partial), (2023, 62, true));
        assert_eq!(december.scrams, by_system(&[(System::Feedwater, 1)]));
        assert_eq!(december.circumstances[Circumstance::Testing.index()], 1);
        assert_eq!(december.lost_kwh[System::Feedwater.index()], 48_000);
        assert_eq!((winter.year, winter.unit_days, winter.capacity_kwh), (2024, 120, 120 * 24_000));
        assert_eq!(winter.scrams, by_system(&[(System::Feedwater, 1), (System::NotNamed, 1)]));
        assert_eq!(winter.circumstances[Circumstance::Unnamed.index()], 2);
        assert_eq!(winter.lost_kwh[System::Feedwater.index()], 24_000);
        // 2 scrams over 120 unit-days.
        let rate = winter.rate(winter.total()).unwrap_or_default();
        assert!((rate - 2.0 / (120.0 / 365.25) * 100.0).abs() < 1e-9, "{rate}");
        let share = winter.share(System::Feedwater).unwrap_or_default();
        assert!((share - 24_000.0 / (120.0 * 24_000.0) * 100.0).abs() < 1e-9, "{share}");
    }

    #[test]
    fn uat_scrams_aggregate_years_follow_the_range() {
        let years = years(&series(), &trends(), Some(day("2024-01-01")), Some(day("2024-01-31")));
        assert_eq!(years.len(), 1);
        assert_eq!((years[0].year, years[0].unit_days, years[0].partial), (2024, 62, true));
        assert_eq!(years[0].scrams, by_system(&[(System::NotNamed, 1)]));
    }

    #[test]
    fn uat_scrams_aggregate_units_give_rates_median_episodes_and_the_leading_named_system() {
        let table = units(&series(), None, None);
        let x = &table[0];
        assert_eq!((x.docket.as_str(), x.name.as_str(), x.unit_days, x.scrams), ("X", "Unit X", 91, 2));
        // A named system leads over not named.
        assert_eq!((x.leading, x.median_days, x.lost_kwh), (Some(System::Feedwater), Some(2.0), 48_000));
        let y = &table[1];
        // The March scram falls outside the exposure.
        assert_eq!((y.scrams, y.leading, y.median_days, y.lost_kwh), (1, Some(System::Feedwater), Some(4.0), 24_000));
        let ranged = units(&series(), Some(day("2024-01-01")), Some(day("2024-01-31")));
        assert_eq!((ranged[0].unit_days, ranged[0].scrams, ranged[0].leading, ranged[0].median_days), (31, 1, None, None));
        assert_eq!((ranged[1].unit_days, ranged[1].scrams), (31, 0));
    }

    #[test]
    fn slugs_round_trip_and_unknowns_are_none() {
        for system in System::ALL {
            assert_eq!(System::from_slug(system.slug()), Some(system));
            assert_eq!(System::ALL[system.index()], system);
        }
        for circumstance in Circumstance::ALL {
            assert_eq!(Circumstance::from_slug(circumstance.slug()), Some(circumstance));
            assert_eq!(Circumstance::ALL[circumstance.index()], circumstance);
        }
        assert_eq!(System::from_slug("fire"), None);
        assert_eq!(Circumstance::from_slug("unnamed"), None);
    }

    #[test]
    fn a_scram_serializes_with_its_tags_flat() {
        let scram = Scram {
            number: 45290,
            docket: "05000296".to_owned(),
            date: NaiveDate::from_ymd_opt(2009, 8, 24).unwrap_or_default(),
            kind: ScramKind::Manual,
            initial_power: 100,
            tags: Tags::default(),
            episode: None,
        };
        let json = serde_json::to_value(&scram).unwrap_or_default();
        assert_eq!(json["system"], "not_named");
        assert_eq!(json["circumstance"], "none");
        assert_eq!(json["kind"], "manual");
    }
}
