//! The internal scram check (DESIGN.md Decision 33, PRD-0003 T-007): the NRC's two sources, its
//! Event Notifications and its status reports, held against each other unit by unit.
//!
//! - An **EN scram** is a unit an event's table gives a SCRAM Code other than `N`. The status
//!   reports should show the unit's power drop the day of the event or the next, or name the scram.
//! - A **report scram** is the first of a run of days a unit's status report marks a scram, with
//!   its Scrams (#) count (filled in from 2018) or a reason tag naming the `scram` cause. An EN
//!   should report it within [`WINDOW`] days.
//!
//! A retracted event counts toward neither side (Decision 30): the NRC withdrew it. A report scram
//! only a retracted event matches is a known difference, outside the agreement rate.

use std::collections::HashMap;

use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

pub use downpower_core::scrams::{Counts, EventCounts, ReportCounts};

use crate::events::{EventSet, Scram};
use crate::facilities::Identity;
use crate::ledger::{Source, UnitDay};

/// The fine cause in `catalog/tags.toml` a reason tag names for a scram.
const SCRAM: &str = "scram";

/// How many points below the power it scrammed from a status report must show a unit, the day of
/// its event or the next, for the drop to show. A scram from under this power can't show one.
pub const DROP: u8 = 10;

/// How many days an event may fall either side of a report scram's date.
pub const WINDOW: Days = Days::new(2);

/// How far back a report's Down date may lie and still date the scram it marks; an older one dates
/// an outage the scram happened during.
const DOWN_BACK: Days = Days::new(7);

/// What the status reports show of an EN scram, in the order the check tries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shown {
    /// The unit's power at least [`DROP`] points below its initial power, the day of the event or
    /// the next.
    Dropped,
    /// No such drop, but a report those days names a scram or cites the event.
    Named,
    /// No report lists the unit either day.
    NoReport,
    /// Scrammed from under [`DROP`]% power, where no drop can show.
    LowPower,
    /// None of these: the two sources disagree.
    Missed,
}

/// What the Event Notifications hold for a report scram, in the order the check tries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Found {
    /// A live event on the unit, within [`WINDOW`] days or cited by the report, gives it a SCRAM
    /// Code other than `N`.
    Event,
    /// Only retracted events do: a known difference.
    Retracted,
    /// Events on the unit within the window, none with a scram.
    NoScramCode,
    /// No event on the unit within the window.
    Missed,
}

/// An EN scram and what the status reports show of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventScram {
    pub number: u32,
    pub docket: String,
    pub date: NaiveDate,
    pub scram: Scram,
    pub initial_power: u8,
    /// The unit's reported power the day of the event and the next.
    pub powers: [Option<u8>; 2],
    pub outcome: Shown,
}

/// A report scram and what the Event Notifications hold for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportScram {
    pub docket: String,
    /// The Down date of the first report marking it, or that report's own date.
    pub date: NaiveDate,
    /// The first report marking it.
    pub reported: NaiveDate,
    pub reason: Option<String>,
    /// The events on the unit within the window or cited, in number order.
    pub events: Vec<u32>,
    pub outcome: Found,
}

/// Every case the check found, EN scrams by event number and report scrams by docket and date.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cases {
    pub events: Vec<EventScram>,
    pub reports: Vec<ReportScram>,
}

impl Cases {
    #[must_use]
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for case in &self.events {
            let events = &mut counts.events;
            *match case.outcome {
                Shown::Dropped => &mut events.dropped,
                Shown::Named => &mut events.named,
                Shown::NoReport => &mut events.no_report,
                Shown::LowPower => &mut events.low_power,
                Shown::Missed => &mut events.missed,
            } += 1;
        }
        for case in &self.reports {
            let reports = &mut counts.reports;
            *match case.outcome {
                Found::Event => &mut reports.event,
                Found::Retracted => &mut reports.retracted,
                Found::NoScramCode => &mut reports.no_scram_code,
                Found::Missed => &mut reports.missed,
            } += 1;
        }
        counts
    }
}

/// One unit's line in an event's unit table.
#[derive(Debug, Clone, Copy)]
struct Listed {
    number: u32,
    date: NaiveDate,
    scram: Option<Scram>,
    retracted: bool,
}

impl Listed {
    fn scrammed(&self) -> bool {
        self.scram.is_some_and(|scram| scram != Scram::No)
    }
}

/// An EN scram, waiting on the unit's reports for the day of its event and the next.
#[derive(Debug, Clone)]
struct Pending<'a> {
    docket: &'a str,
    number: u32,
    date: NaiveDate,
    scram: Scram,
    initial_power: u8,
    /// Each day's power, and whether its report names a scram (a scram tag, the Scrams (#)
    /// count, or a citation of the event).
    seen: [Option<(u8, bool)>; 2],
}

/// The check, fed a build's rows one date at a time, in date order.
#[derive(Debug)]
pub struct Check<'a> {
    /// Each docket's lines in event unit tables.
    listed: HashMap<&'a str, Vec<Listed>>,
    scrams: Vec<Pending<'a>>,
    /// The EN scrams a unit's report on a date answers, and for which of their two days.
    wanted: HashMap<(&'a str, NaiveDate), Vec<(usize, usize)>>,
    /// The last date each unit's report marked a scram.
    marked: HashMap<String, NaiveDate>,
    reports: Vec<ReportScram>,
}

impl<'a> Check<'a> {
    #[must_use]
    pub fn new(events: &'a EventSet) -> Self {
        let mut listed: HashMap<&'a str, Vec<Listed>> = HashMap::new();
        let mut scrams = Vec::new();
        let mut wanted: HashMap<(&'a str, NaiveDate), Vec<(usize, usize)>> = HashMap::new();
        for (&number, event) in &events.events {
            let Some(Identity::Reactor { dockets }) = events.identities.get(&number) else { continue };
            for (unit, docket) in event.units.iter().zip(dockets) {
                let docket = docket.as_str();
                let date = event.occurred.date;
                if let Some(scram) = unit.scram.filter(|scram| *scram != Scram::No)
                    && !event.retracted
                {
                    for (slot, day) in [date, date + Days::new(1)].into_iter().enumerate() {
                        wanted.entry((docket, day)).or_default().push((scrams.len(), slot));
                    }
                    scrams.push(Pending {
                        docket,
                        number,
                        date,
                        scram,
                        initial_power: unit.initial_power,
                        seen: [None; 2],
                    });
                }
                listed.entry(docket).or_default().push(Listed {
                    number,
                    date,
                    scram: unit.scram,
                    retracted: event.retracted,
                });
            }
        }
        Self {
            listed,
            scrams,
            wanted,
            marked: HashMap::new(),
            reports: Vec::new(),
        }
    }

    /// Takes one date's rows. Only rows from an NRC page count: EIA's say nothing of scrams.
    pub fn observe(&mut self, days: &[UnitDay]) {
        for day in days.iter().filter(|day| day.source == Source::NrcPage) {
            let names_scram = day.scrams > 0 || day.causes.iter().any(|cause| cause.cause == SCRAM);
            for &(index, slot) in self.wanted.get(&(day.docket.as_str(), day.date)).into_iter().flatten() {
                let pending = &mut self.scrams[index];
                pending.seen[slot] = Some((day.power, names_scram || day.xrefs.contains(&Some(pending.number))));
            }
            if names_scram {
                let previous = self.marked.insert(day.docket.clone(), day.date);
                if previous.is_none_or(|previous| previous + Days::new(1) != day.date) {
                    let report = self.report(day);
                    self.reports.push(report);
                }
            }
        }
    }

    /// Classifies a report scram by the events on its unit.
    fn report(&self, day: &UnitDay) -> ReportScram {
        let date = day.down.filter(|down| *down <= day.date && day.date - DOWN_BACK <= *down).unwrap_or(day.date);
        let near = |line: &&Listed| (date - WINDOW <= line.date && line.date <= date + WINDOW) || day.xrefs.contains(&Some(line.number));
        let lines: Vec<&Listed> = self.listed.get(day.docket.as_str()).into_iter().flatten().filter(near).collect();
        let outcome = if lines.iter().any(|line| line.scrammed() && !line.retracted) {
            Found::Event
        } else if lines.iter().any(|line| line.scrammed()) {
            Found::Retracted
        } else if lines.is_empty() {
            Found::Missed
        } else {
            Found::NoScramCode
        };
        let mut events: Vec<u32> = lines.iter().map(|line| line.number).collect();
        events.sort_unstable();
        events.dedup();
        ReportScram {
            docket: day.docket.clone(),
            date,
            reported: day.date,
            reason: day.reason.clone(),
            events,
            outcome,
        }
    }

    /// Classifies every EN scram by the reports seen.
    #[must_use]
    pub fn finish(self) -> Cases {
        let events = self
            .scrams
            .into_iter()
            .map(|pending| {
                let powers = pending.seen.map(|seen| seen.map(|(power, _)| power));
                let outcome = if powers.iter().flatten().any(|power| power.saturating_add(DROP) <= pending.initial_power) {
                    Shown::Dropped
                } else if pending.seen.iter().flatten().any(|&(_, names)| names) {
                    Shown::Named
                } else if pending.seen.iter().all(Option::is_none) {
                    Shown::NoReport
                } else if pending.initial_power < DROP {
                    Shown::LowPower
                } else {
                    Shown::Missed
                };
                EventScram {
                    number: pending.number,
                    docket: pending.docket.to_owned(),
                    date: pending.date,
                    scram: pending.scram,
                    initial_power: pending.initial_power,
                    powers,
                    outcome,
                }
            })
            .collect();
        let mut reports = self.reports;
        reports.sort_by(|a, b| (&a.docket, a.date).cmp(&(&b.docket, b.date)));
        Cases { events, reports }
    }
}
