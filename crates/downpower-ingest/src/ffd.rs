//! Fitness for duty (DESIGN.md Decision 36, PRD-0005 T-001 and T-002): which events are FFD events,
//! and what each is about. An event is one when a 10 CFR section it carries is 26.73, 26.719 or
//! 26.417(b)(1), or when `catalog/ffd.toml` lists it, and not when that file excludes it or the NRC
//! retracted it. Never by its title or a bare keyword: titles vary, and keywords misfire. Its tags
//! come from the same file's ordered phrase rules.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context as _, ensure};
use chrono::Datelike as _;
use regex::Regex;
use serde::{Deserialize, Serialize};

use downpower_core::ffd::{Event as FfdEvent, Exposure, Law as CoreLaw};
pub use downpower_core::ffd::{Kind, Role, Series, Site, Substance, Tags, Test};

use crate::Res;
use crate::cannabis::Laws;
use crate::events::{Category, Event, EventSet};
use crate::facilities::Identity;
use crate::ledger::UnitDay;
use crate::units::{Docket, Unit, Units};

/// The reviewed exceptions and, later, the phrase rules.
pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/ffd.toml"));

/// The sections that make an event an FFD event.
pub const SECTIONS: [&str; 3] = ["26.73", "26.719", "26.417(b)(1)"];

/// Why an event is an FFD event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// A section it carries, as [`SECTIONS`] writes it.
    Section(String),
    /// `catalog/ffd.toml`'s include list.
    Listed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    include: Vec<Listed>,
    exclude: Vec<Listed>,
    strip: Vec<String>,
    substance: Vec<RuleRow<Substance>>,
    role: Vec<RuleRow<Role>>,
    kind: Vec<RuleRow<Kind>>,
    test: Vec<RuleRow<Test>>,
}

/// A phrase rule as the file writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleRow<T> {
    value: T,
    pattern: String,
}

/// One dimension's rules, in order: the first whose pattern matches gives the value.
#[derive(Debug, Clone)]
struct Rules<T>(Vec<(T, Regex)>);

impl<T: Copy + std::fmt::Debug> Rules<T> {
    fn compile(rows: Vec<RuleRow<T>>) -> Res<Self> {
        rows.into_iter()
            .map(|row| Ok((row.value, Regex::new(&row.pattern).with_context(|| format!("the pattern for {:?}", row.value))?)))
            .collect::<Res<_>>()
            .map(Self)
    }

    fn first(&self, text: &str) -> Option<T> {
        self.0.iter().find(|(_, pattern)| pattern.is_match(text)).map(|(value, _)| *value)
    }
}

/// A reviewed exception: the event and why.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
    number: u32,
    why: String,
}

/// The identification and phrase rules.
#[derive(Debug, Clone)]
pub struct Ffd {
    /// Each listed event, included (`true`) or excluded.
    listed: BTreeMap<u32, bool>,
    strip: Vec<Regex>,
    substance: Rules<Substance>,
    role: Rules<Role>,
    kind: Rules<Kind>,
    test: Rules<Test>,
}

impl Ffd {
    /// The committed rules.
    ///
    /// # Errors
    ///
    /// When `catalog/ffd.toml` breaks a rule [`Ffd::parse`] checks.
    pub fn catalog() -> Res<Self> {
        Self::parse(CATALOG).context("catalog/ffd.toml")
    }

    /// Parses an FFD catalog.
    ///
    /// # Errors
    ///
    /// When the TOML doesn't parse, an exception gives no reason, an event is listed twice, or a
    /// pattern doesn't compile.
    pub fn parse(toml: &str) -> Res<Self> {
        let file: File = toml::from_str(toml).context("parsing the FFD catalog")?;
        let mut listed = BTreeMap::new();
        for (include, entries) in [(true, file.include), (false, file.exclude)] {
            for entry in entries {
                ensure!(!entry.why.trim().is_empty(), "EN {} is listed without a reason", entry.number);
                ensure!(listed.insert(entry.number, include).is_none(), "EN {} is listed twice", entry.number);
            }
        }
        let strip = file
            .strip
            .iter()
            .map(|pattern| Regex::new(pattern).with_context(|| format!("the strip pattern {pattern:?}")))
            .collect::<Res<_>>()?;
        Ok(Self {
            listed,
            strip,
            substance: Rules::compile(file.substance)?,
            role: Rules::compile(file.role)?,
            kind: Rules::compile(file.kind)?,
            test: Rules::compile(file.test)?,
        })
    }

    /// Why `event` is an FFD event, or `None` when it isn't one or was retracted.
    #[must_use]
    pub fn identify(&self, event: &Event) -> Option<Why> {
        if event.retracted {
            return None;
        }
        match self.listed.get(&event.number) {
            Some(true) => Some(Why::Listed),
            Some(false) => None,
            None => event
                .sections
                .iter()
                .find_map(|found| found.code.as_deref().and_then(section))
                .map(|code| Why::Section(code.to_owned())),
        }
    }
}

impl Ffd {
    /// `event`'s tags: its title and text lowercased, whitespace collapsed and boilerplate blanked,
    /// then each dimension's first matching rule, or none named.
    #[must_use]
    pub fn tags(&self, event: &Event) -> Tags {
        let mut text = format!("{} {}", event.title.as_deref().unwrap_or_default(), event.text)
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for pattern in &self.strip {
            text = pattern.replace_all(&text, " ").into_owned();
        }
        let none = Tags::default();
        Tags {
            substance: self.substance.first(&text).unwrap_or(none.substance),
            role: self.role.first(&text).unwrap_or(none.role),
            kind: self.kind.first(&text).unwrap_or(none.kind),
            test: self.test.first(&text).unwrap_or(none.test),
        }
    }
}

/// The [`SECTIONS`] entry `code` falls under, spaces ignored: the code is the section or one of
/// its paragraphs, so `26.719 (b)(2)` is 26.719 and `26.7190` would be no section of it.
#[must_use]
pub fn section(code: &str) -> Option<&'static str> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    SECTIONS
        .into_iter()
        .find(|section| code.strip_prefix(section).is_some_and(|rest| rest.is_empty() || rest.starts_with('(')))
}

/// The FFD series a rebuild collects (PRD-0005 T-005 and T-006): every unretracted FFD event with
/// its tags and where it happened, and each state's operating exposure by year from the rows.
#[derive(Debug)]
pub struct Collector<'a> {
    /// Each catalog unit's state, by docket.
    states: HashMap<&'a str, &'a str>,
    laws: &'a Laws,
    exposure: BTreeMap<(String, i32), Exposure>,
    series: Series,
}

/// Where `event` happened: at an operating reactor when a unit it names was in service that day,
/// at a construction site when every unit it names was before commercial operation or holds a
/// Part 52 combined license the catalog has no unit for (a unit that never operated, such as
/// Summer 2 and 3), at another reactor site otherwise, and elsewhere when it isn't a power reactor
/// event.
fn site(event: &Event, identity: Option<&Identity>, units: &HashMap<&str, &Unit>) -> Site {
    if event.category != Category::PowerReactor {
        return Site::Facility;
    }
    let Some(Identity::Reactor { dockets }) = identity else { return Site::Reactor };
    let date = event.occurred.date;
    let unit = |docket: &Docket| units.get(docket.as_str()).copied();
    if dockets.iter().any(|docket| unit(docket).is_some_and(|unit| unit.in_service(date))) {
        Site::Operating
    } else if !dockets.is_empty()
        && dockets.iter().all(|docket| match unit(docket) {
            Some(unit) => unit.commercial.is_some_and(|commercial| date < commercial),
            None => docket.as_str().starts_with("052"),
        })
    {
        Site::Construction
    } else {
        Site::Reactor
    }
}

impl<'a> Collector<'a> {
    /// A collector holding `events`' FFD events, as `ffd` identifies and tags them.
    #[must_use]
    pub fn new(events: &EventSet, units: &'a Units, ffd: &Ffd, laws: &'a Laws) -> Self {
        let by_docket: HashMap<&str, &Unit> = units.all().iter().map(|unit| (unit.docket.as_str(), unit)).collect();
        let found = events
            .events
            .iter()
            .filter(|(_, event)| ffd.identify(event).is_some())
            .map(|(number, event)| {
                let identity = events.identities.get(number);
                let first_unit = match identity {
                    Some(Identity::Reactor { dockets }) => dockets.first().and_then(|docket| by_docket.get(docket.as_str())).map(|unit| unit.state.clone()),
                    _ => None,
                };
                FfdEvent {
                    number: *number,
                    date: event.occurred.date,
                    state: event.state.clone().or(first_unit),
                    site: site(event, identity, &by_docket),
                    tags: ffd.tags(event),
                }
            })
            .collect();
        let series = Series {
            events: found,
            laws: laws
                .iter()
                .map(|law| CoreLaw {
                    state: law.state.clone(),
                    adult_use: law.adult_use,
                    sales: law.sales,
                    medical: law.medical,
                })
                .collect(),
            ..Series::default()
        };
        Self {
            states: units.all().iter().map(|unit| (unit.docket.as_str(), unit.state.as_str())).collect(),
            laws,
            exposure: BTreeMap::new(),
            series,
        }
    }

    /// Takes one date's rows: each is an operating unit-day in its unit's state, legal there or not.
    pub fn push(&mut self, days: &[UnitDay]) {
        for day in days {
            let Some(state) = self.states.get(day.docket.as_str()) else { continue };
            let law = self.laws.get(state);
            let exposure = self.exposure.entry(((*state).to_owned(), day.date.year())).or_insert_with(|| Exposure {
                state: (*state).to_owned(),
                year: day.date.year(),
                unit_days: 0,
                adult_use_days: 0,
                medical_days: 0,
            });
            exposure.unit_days += 1;
            exposure.adult_use_days += u32::from(law.is_some_and(|law| law.adult_use_on(day.date)));
            exposure.medical_days += u32::from(law.is_some_and(|law| law.medical_on(day.date)));
            self.series.first = self.series.first.min(Some(day.date)).or(Some(day.date));
            self.series.last = self.series.last.max(Some(day.date));
        }
    }

    /// The series: the events oldest first, the exposure by state and year.
    #[must_use]
    pub fn finish(mut self) -> Series {
        self.series.events.sort_by_key(|event| (event.date, event.number));
        self.series.exposure = self.exposure.into_values().collect();
        self.series
    }
}
