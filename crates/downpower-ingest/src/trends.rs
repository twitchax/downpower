//! The fleet trends a rebuild collects beside the monthly series (PRD-0006 T-002): each month's
//! in-service unit-days and capacity energy, the Event Notification scrams by kind, and every
//! refueling outage, read from the rows in date order.

use std::collections::{HashMap, HashSet};

use chrono::{Days, NaiveDate};

use downpower_core::Family;
use downpower_core::trends::{Outage, ScramKind, Trends};

use crate::Res;
use crate::events::{EventSet, Scram};
use crate::facilities::Identity;
use crate::ledger::UnitDay;
use crate::units::Units;

/// A unit's run of days at 0% power.
#[derive(Debug, Clone, Copy)]
struct Run {
    start: NaiveDate,
    days: u32,
    /// Days by attributed family, by [`Family::index`].
    families: [u32; 8],
    /// Whether the unit's day before it was at power.
    after_power: bool,
}

impl Run {
    /// Whether Refueling is the run's most common attributed family; a tie counts.
    fn refueling(&self) -> bool {
        let refueling = self.families[usize::from(Family::Refueling.index())];
        refueling > 0 && self.families.iter().all(|days| *days <= refueling)
    }
}

/// What the collector remembers of a unit: its last row's date and power, and the run it is in.
#[derive(Debug, Clone, Copy)]
struct Last {
    date: NaiveDate,
    at_power: bool,
    run: Option<Run>,
}

/// The trends so far, fed one date's rows at a time.
#[derive(Debug, Default)]
pub struct Collector {
    trends: Trends,
    /// Every unretracted EN scram on a catalog unit, waiting for the months to cover it.
    scrams: Vec<(NaiveDate, ScramKind)>,
    units: HashMap<String, Last>,
}

impl Collector {
    /// A collector that counts `events`' scrams on the catalog's units: each unit of an
    /// unretracted reactor event with a SCRAM Code other than `N`, in the month the event occurred.
    #[must_use]
    pub fn new(events: &EventSet, units: &Units) -> Self {
        let catalog: HashSet<&str> = units.all().iter().map(|unit| unit.docket.as_str()).collect();
        let mut scrams = Vec::new();
        for (number, event) in events.events.iter().filter(|(_, event)| !event.retracted) {
            let Some(Identity::Reactor { dockets }) = events.identities.get(number) else { continue };
            for (unit, docket) in event.units.iter().zip(dockets) {
                let kind = match unit.scram {
                    Some(Scram::AutoReactor | Scram::Auto) => ScramKind::Automatic,
                    Some(Scram::ManualReactor | Scram::Manual) => ScramKind::Manual,
                    Some(Scram::No) | None => continue,
                };
                if catalog.contains(docket.as_str()) {
                    scrams.push((event.occurred.date, kind));
                }
            }
        }
        Self { scrams, ..Self::default() }
    }

    /// Takes one date's rows: each counts toward its month's exposure and capacity, and moves its
    /// unit's run of days at 0%. A run that ends on a day at power, with a day at power the day
    /// before it began and no day missing, is an outage, kept when mostly Refueling.
    ///
    /// # Errors
    ///
    /// When the rows go back before the first month.
    pub fn push(&mut self, days: &[UnitDay]) -> Res<()> {
        for day in days {
            self.trends.add_day(day.date, day.capacity_kw)?;
            let last = self.units.get(&day.docket).copied();
            let follows = last.is_some_and(|last| last.date + Days::new(1) == day.date);
            let run = last.and_then(|last| last.run).filter(|_| follows);
            let run = if day.power == 0 {
                let mut run = run.unwrap_or(Run {
                    start: day.date,
                    days: 0,
                    families: [0; 8],
                    after_power: follows && last.is_some_and(|last| last.at_power),
                });
                run.days += 1;
                run.families[usize::from(day.family.index())] += 1;
                Some(run)
            } else {
                if let Some(run) = run.filter(|run| run.after_power && run.refueling()) {
                    self.trends.outages.push(Outage {
                        docket: day.docket.clone(),
                        start: run.start,
                        days: run.days,
                    });
                }
                None
            };
            self.units.insert(
                day.docket.clone(),
                Last {
                    date: day.date,
                    at_power: day.power > 0,
                    run,
                },
            );
        }
        Ok(())
    }

    /// The trends, with the scrams the months cover and the outages by start date. A run still
    /// open at the last rows isn't an outage yet.
    #[must_use]
    pub fn finish(mut self) -> Trends {
        for (date, kind) in self.scrams {
            self.trends.add_scram(date, kind);
        }
        self.trends.outages.sort_by(|a, b| (a.start, &a.docket).cmp(&(b.start, &b.docket)));
        self.trends
    }
}
