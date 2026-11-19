//! Fitness for duty (DESIGN.md Decisions 36 to 38, PRD-0005): what an FFD event is about, as the
//! ordered phrase rules in `catalog/ffd.toml` tag it, and the series a rebuild stores for `/ffd`.
//! Each tag dimension has a value for "none named", so a missing tag shows rather than dropping
//! out. Rates are per 100 operating unit-years, and events at construction sites and elsewhere
//! count apart (Decision 38).

use std::collections::BTreeMap;

use chrono::{Datelike as _, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::trends::DAYS_PER_YEAR;

/// The substance an event names: named drugs first, then generic drug wording, then alcohol, so an
/// event naming a drug and alcohol counts as drug-related.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Substance {
    Marijuana,
    Cocaine,
    Opiates,
    Amphetamines,
    OtherDrug,
    /// Generic wording: "illegal drug", "controlled substance", paraphernalia.
    Drug,
    Alcohol,
    #[serde(rename = "none")]
    Unnamed,
}

impl Substance {
    pub const ALL: [Self; 8] = [
        Self::Marijuana,
        Self::Cocaine,
        Self::Opiates,
        Self::Amphetamines,
        Self::OtherDrug,
        Self::Drug,
        Self::Alcohol,
        Self::Unnamed,
    ];

    /// A named drug, generic drug wording or a drug found (PRD-0005 T-002); alcohol and nothing
    /// named stay apart.
    #[must_use]
    pub const fn drug_related(self) -> bool {
        !matches!(self, Self::Alcohol | Self::Unnamed)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Marijuana => "Marijuana",
            Self::Cocaine => "Cocaine",
            Self::Opiates => "Opiates",
            Self::Amphetamines => "Amphetamines",
            Self::OtherDrug => "Other named drug",
            Self::Drug => "Drug, unnamed",
            Self::Alcohol => "Alcohol",
            Self::Unnamed => "None named",
        }
    }
}

/// Whose test, refusal or violation it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    LicensedOperator,
    Supervisor,
    FfdStaff,
    Security,
    Contractor,
    Employee,
    #[serde(rename = "none")]
    Unnamed,
}

impl Role {
    pub const ALL: [Self; 7] = [
        Self::LicensedOperator,
        Self::Supervisor,
        Self::FfdStaff,
        Self::Security,
        Self::Contractor,
        Self::Employee,
        Self::Unnamed,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LicensedOperator => "Licensed operator",
            Self::Supervisor => "Supervisor",
            Self::FfdStaff => "FFD program staff",
            Self::Security => "Security",
            Self::Contractor => "Contractor",
            Self::Employee => "Employee",
            Self::Unnamed => "None named",
        }
    }
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A program or laboratory failure: blind samples, testing pools, access granted untested.
    ProgramFailure,
    /// A substance or container found in the protected area.
    SubstanceFound,
    /// Subverting a test: adulterated or substituted samples.
    Subversion,
    Refusal,
    /// A positive test.
    Positive,
    /// Another violation: an arrest, an admission, a policy violation.
    Violation,
    #[serde(rename = "none")]
    Unnamed,
}

impl Kind {
    pub const ALL: [Self; 7] = [
        Self::ProgramFailure,
        Self::SubstanceFound,
        Self::Subversion,
        Self::Refusal,
        Self::Positive,
        Self::Violation,
        Self::Unnamed,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProgramFailure => "Program failure",
            Self::SubstanceFound => "Substance found",
            Self::Subversion => "Subversion",
            Self::Refusal => "Refusal",
            Self::Positive => "Positive test",
            Self::Violation => "Other violation",
            Self::Unnamed => "None named",
        }
    }
}

/// The test that found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Test {
    Random,
    ForCause,
    PostEvent,
    FollowUp,
    PreAccess,
    ReturnToDuty,
    #[serde(rename = "none")]
    Unnamed,
}

impl Test {
    pub const ALL: [Self; 7] = [Self::Random, Self::ForCause, Self::PostEvent, Self::FollowUp, Self::PreAccess, Self::ReturnToDuty, Self::Unnamed];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Random => "Random",
            Self::ForCause => "For cause",
            Self::PostEvent => "Post-event",
            Self::FollowUp => "Follow-up",
            Self::PreAccess => "Pre-access",
            Self::ReturnToDuty => "Return to duty",
            Self::Unnamed => "None named",
        }
    }
}

/// An FFD event's tags, one per dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tags {
    pub substance: Substance,
    pub role: Role,
    pub kind: Kind,
    pub test: Test,
}

impl Default for Tags {
    fn default() -> Self {
        Self {
            substance: Substance::Unnamed,
            role: Role::Unnamed,
            kind: Kind::Unnamed,
            test: Test::Unnamed,
        }
    }
}

/// Where an FFD event happened, which decides whether it counts toward the operating rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    /// A reactor unit the event names was in service that day.
    Operating,
    /// Every unit it names was still before commercial operation, or never reached it.
    Construction,
    /// Another reactor event: a shut-down unit, or a site event naming no unit.
    Reactor,
    /// Not a power reactor event: a fuel cycle facility, or another category.
    Facility,
}

/// An FFD event as the series counts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub number: u32,
    /// The day it occurred.
    pub date: NaiveDate,
    /// The plant's state; none for an event the page gives no state.
    pub state: Option<String>,
    pub site: Site,
    #[serde(flatten)]
    pub tags: Tags,
}

/// One state's operating exposure in one year.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exposure {
    pub state: String,
    pub year: i32,
    /// In-service unit-days: the ledger's rows.
    pub unit_days: u32,
    /// Of them, days adult use was legal in the state, and days a comprehensive medical program was.
    pub adult_use_days: u32,
    pub medical_days: u32,
}

/// A state's legalization dates, as `catalog/cannabis.csv` gives them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Law {
    pub state: String,
    pub adult_use: Option<NaiveDate>,
    pub sales: Option<NaiveDate>,
    pub medical: Option<NaiveDate>,
}

/// A build's FFD series, as `/api/ffd` serves it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    /// The first and last days the ledger covers.
    pub first: Option<NaiveDate>,
    pub last: Option<NaiveDate>,
    /// Every unretracted FFD event, oldest first.
    pub events: Vec<Event>,
    /// Operating exposure by state and year.
    pub exposure: Vec<Exposure>,
    /// Every state's dates.
    pub laws: Vec<Law>,
}

/// The day Part 26's 2008 rewrite took effect (73 FR 16966): reports moved from 26.73 to 26.719.
#[must_use]
pub fn rewrite() -> NaiveDate {
    NaiveDate::from_ymd_opt(2008, 4, 30).unwrap_or_default()
}

/// One year of the national overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Year {
    pub year: i32,
    pub unit_days: u32,
    /// Events at operating reactors: all, drug-related and alcohol.
    pub ffd: u32,
    pub drug_related: u32,
    pub alcohol: u32,
    /// Events counted apart: at construction sites, at other reactor sites, and elsewhere.
    pub construction: u32,
    pub reactor: u32,
    pub facility: u32,
    pub adult_use_days: u32,
    pub medical_days: u32,
    /// The ledger covers only part of the year.
    pub partial: bool,
}

impl Year {
    #[must_use]
    pub fn unit_years(&self) -> f64 {
        f64::from(self.unit_days) / DAYS_PER_YEAR
    }

    /// `count` per 100 operating unit-years, none without exposure.
    #[must_use]
    pub fn rate(&self, count: u32) -> Option<f64> {
        (self.unit_days > 0).then(|| f64::from(count) / self.unit_years() * 100.0)
    }

    /// The share of operating unit-days in adult-use states and in medical states, as percentages.
    #[must_use]
    pub fn shares(&self) -> Option<(f64, f64)> {
        let share = |days: u32| f64::from(days) / f64::from(self.unit_days) * 100.0;
        (self.unit_days > 0).then(|| (share(self.adult_use_days), share(self.medical_days)))
    }
}

/// The national overlay, a year per year the ledger covers.
#[must_use]
pub fn years(series: &Series) -> Vec<Year> {
    let (Some(first), Some(last)) = (series.first, series.last) else { return Vec::new() };
    let mut years: BTreeMap<i32, Year> = (first.year()..=last.year())
        .map(|year| {
            let partial = NaiveDate::from_ymd_opt(year, 1, 1).is_none_or(|january| first > january) || NaiveDate::from_ymd_opt(year, 12, 31).is_none_or(|december| last < december);
            let empty = Year {
                year,
                unit_days: 0,
                ffd: 0,
                drug_related: 0,
                alcohol: 0,
                construction: 0,
                reactor: 0,
                facility: 0,
                adult_use_days: 0,
                medical_days: 0,
                partial,
            };
            (year, empty)
        })
        .collect();
    for exposure in &series.exposure {
        if let Some(year) = years.get_mut(&exposure.year) {
            year.unit_days += exposure.unit_days;
            year.adult_use_days += exposure.adult_use_days;
            year.medical_days += exposure.medical_days;
        }
    }
    for event in &series.events {
        let Some(year) = years.get_mut(&event.date.year()) else { continue };
        match event.site {
            Site::Operating => {
                year.ffd += 1;
                year.drug_related += u32::from(event.tags.substance.drug_related());
                year.alcohol += u32::from(event.tags.substance == Substance::Alcohol);
            }
            Site::Construction => year.construction += 1,
            Site::Reactor => year.reactor += 1,
            Site::Facility => year.facility += 1,
        }
    }
    years.into_values().collect()
}

/// One state's row of the state table.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub state: String,
    pub unit_days: u32,
    pub ffd: u32,
    pub drug_related: u32,
    /// Drug-related events and operating unit-days before adult use took effect, and since.
    pub before: (u32, u32),
    pub after: (u32, u32),
    pub law: Option<Law>,
}

/// `code`'s row of the state table, added empty with its law the first time.
fn row<'a>(states: &'a mut BTreeMap<String, State>, code: &str, law: Option<&Law>) -> &'a mut State {
    states.entry(code.to_owned()).or_insert_with(|| State {
        state: code.to_owned(),
        unit_days: 0,
        ffd: 0,
        drug_related: 0,
        before: (0, 0),
        after: (0, 0),
        law: law.cloned(),
    })
}

/// The state table: every state with operating exposure or an operating-site event, by code.
#[must_use]
pub fn states(series: &Series) -> Vec<State> {
    let law = |code: &str| series.laws.iter().find(|law| law.state == code);
    let mut states: BTreeMap<String, State> = BTreeMap::new();
    for exposure in &series.exposure {
        let state = row(&mut states, &exposure.state, law(&exposure.state));
        state.unit_days += exposure.unit_days;
        state.before.1 += exposure.unit_days.saturating_sub(exposure.adult_use_days);
        state.after.1 += exposure.adult_use_days;
    }
    for event in series.events.iter().filter(|event| event.site == Site::Operating) {
        let Some(code) = event.state.as_deref() else { continue };
        let legal = law(code).and_then(|law| law.adult_use).is_some_and(|since| since <= event.date);
        let state = row(&mut states, code, law(code));
        state.ffd += 1;
        if event.tags.substance.drug_related() {
            state.drug_related += 1;
            if legal {
                state.after.0 += 1;
            } else {
                state.before.0 += 1;
            }
        }
    }
    states.into_values().collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    fn event(number: u32, date: &str, state: &str, site: Site, substance: Substance) -> Event {
        Event {
            number,
            date: day(date),
            state: Some(state.to_owned()),
            site,
            tags: Tags { substance, ..Tags::default() },
        }
    }

    /// Two states over 2022 to mid-2023: Illinois legal from 2020, Georgia never.
    fn series() -> Series {
        let exposure = |state: &str, year: i32, unit_days: u32, legal: u32| Exposure {
            state: state.to_owned(),
            year,
            unit_days,
            adult_use_days: legal,
            medical_days: legal,
        };
        Series {
            first: Some(day("2022-01-01")),
            last: Some(day("2023-06-30")),
            events: vec![
                event(1, "2022-03-01", "IL", Site::Operating, Substance::Marijuana),
                event(2, "2022-05-01", "GA", Site::Operating, Substance::Alcohol),
                event(3, "2022-06-01", "GA", Site::Construction, Substance::Drug),
                event(4, "2023-02-01", "GA", Site::Operating, Substance::Drug),
                event(5, "2023-03-01", "TN", Site::Facility, Substance::Unnamed),
            ],
            exposure: vec![
                exposure("IL", 2022, 3_653, 3_653),
                exposure("GA", 2022, 3_653, 0),
                exposure("IL", 2023, 1_000, 1_000),
                exposure("GA", 2023, 1_000, 0),
            ],
            laws: vec![
                Law {
                    state: "IL".to_owned(),
                    adult_use: Some(day("2020-01-01")),
                    sales: Some(day("2020-01-01")),
                    medical: Some(day("2014-01-01")),
                },
                Law {
                    state: "GA".to_owned(),
                    adult_use: None,
                    sales: None,
                    medical: None,
                },
            ],
        }
    }

    #[test]
    fn uat_ffd_series_rates_shares_and_events_apart() {
        let years = years(&series());
        assert_eq!(years.iter().map(|year| (year.year, year.partial)).collect::<Vec<_>>(), [(2022, false), (2023, true)]);
        let year = years[0];
        // 7,306 unit-days is 20.0 unit-years: two operating events, one drug-related.
        assert_eq!((year.ffd, year.drug_related, year.alcohol, year.construction, year.facility), (2, 1, 1, 1, 0));
        assert!(year.rate(year.ffd).is_some_and(|rate| (rate - 10.0).abs() < 0.01));
        assert!(year.rate(year.drug_related).is_some_and(|rate| (rate - 5.0).abs() < 0.01));
        assert_eq!(year.shares(), Some((50.0, 50.0)));
        assert_eq!((years[1].ffd, years[1].facility), (1, 1));
    }

    #[test]
    fn uat_ffd_series_states_split_at_adult_use() {
        let states = states(&series());
        assert_eq!(states.iter().map(|state| state.state.as_str()).collect::<Vec<_>>(), ["GA", "IL"]);
        let georgia = &states[0];
        // The construction-site event counts apart.
        assert_eq!((georgia.unit_days, georgia.ffd, georgia.drug_related), (4_653, 2, 1));
        assert_eq!((georgia.before, georgia.after), ((1, 4_653), (0, 0)), "never legal: all before");
        let illinois = &states[1];
        assert_eq!((illinois.before, illinois.after), ((0, 0), (1, 4_653)));
        assert_eq!(illinois.law.as_ref().and_then(|law| law.adult_use), Some(day("2020-01-01")));
    }

    #[test]
    fn drug_related_leaves_out_alcohol_and_nothing_named() {
        let related: Vec<Substance> = Substance::ALL.into_iter().filter(|substance| substance.drug_related()).collect();
        assert_eq!(
            related,
            [
                Substance::Marijuana,
                Substance::Cocaine,
                Substance::Opiates,
                Substance::Amphetamines,
                Substance::OtherDrug,
                Substance::Drug
            ]
        );
    }

    #[test]
    fn none_named_serializes_as_none() -> crate::Res<()> {
        assert_eq!(serde_json::to_string(&Tags::default())?, r#"{"substance":"none","role":"none","kind":"none","test":"none"}"#);
        assert_eq!(serde_json::to_string(&Substance::OtherDrug)?, r#""other_drug""#);
        Ok(())
    }
}
