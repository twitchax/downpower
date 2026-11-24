//! Scram episodes (DESIGN.md Decision 42, PRD-0007 T-003): what each Event Notification scram cost.
//! An episode is the unit's run of ledger rows from the drop, the day of the event or the next
//! (the scram check's rule, Decision 33), until the unit is back within [`DROP`] points of the
//! power it scrammed from. A day attributed to Refueling ends it, so a scram just before an outage
//! doesn't book the outage, and so does the unit's next scram that can have an episode of its own.
//! Its energy is measured below the pre-scram power, whatever cause each row is attributed to, and
//! no row's attribution changes.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike as _, Days, NaiveDate};

use downpower_core::Family;
use downpower_core::trends::ScramKind;
pub use downpower_core::triggers::{DROP, End, Episode, Scram, Series, Tags, UnitExposure};

use crate::events::{Event, EventSet, Scram as Code};
use crate::facilities::Identity;
use crate::ledger::UnitDay;
use crate::units::Units;

/// Where a scram's episode stands.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Waiting on the reports for the day of the event and the next to show the drop.
    Waiting,
    /// Down since `episode.start`, last counted on `last`.
    Running { episode: Episode, last: NaiveDate },
    /// Settled: the episode, or none.
    Done,
}

/// One docket's scrams, by date, and where the rows have reached in them.
#[derive(Debug, Default)]
struct Unit {
    /// Indices into the collector's scrams, oldest first.
    order: Vec<usize>,
    /// The first of `order` the rows haven't reached.
    next: usize,
    /// The scram whose episode is running.
    running: Option<usize>,
    /// Scrams reached and waiting on their two days.
    waiting: Vec<usize>,
}

/// The scram series: every scram's episode, and each catalog unit's in-service days by month, fed
/// a build's rows one date at a time, in date order.
#[derive(Debug, Default)]
pub struct Collector {
    scrams: Vec<(Scram, State)>,
    units: HashMap<String, Unit>,
    /// Each catalog unit's name, by docket.
    names: HashMap<String, String>,
    /// Each unit's rows per month from `first`, by docket.
    days: BTreeMap<String, Vec<u8>>,
    /// The first day of the first row's month, and the last row's day.
    first: Option<NaiveDate>,
    last: Option<NaiveDate>,
}

/// Whether a unit at `power` is down from a scram from `initial`.
const fn down(power: u8, initial: u8) -> bool {
    power.saturating_add(DROP) <= initial
}

impl Collector {
    /// A collector for `events`' scrams on the catalog's units: each unit of an unretracted reactor
    /// event with a SCRAM Code other than `N`, with the tags `tag` gives its event.
    #[must_use]
    pub fn new(events: &EventSet, units: &Units, tag: impl Fn(&Event) -> Tags) -> Self {
        let catalog: HashSet<&str> = units.all().iter().map(|unit| unit.docket.as_str()).collect();
        let mut scrams = Vec::new();
        for (number, event) in events.events.iter().filter(|(_, event)| !event.retracted) {
            let Some(Identity::Reactor { dockets }) = events.identities.get(number) else { continue };
            let mut tags = None;
            for (unit, docket) in event.units.iter().zip(dockets) {
                let kind = match unit.scram {
                    Some(Code::AutoReactor | Code::Auto) => ScramKind::Automatic,
                    Some(Code::ManualReactor | Code::Manual) => ScramKind::Manual,
                    Some(Code::No) | None => continue,
                };
                if !catalog.contains(docket.as_str()) {
                    continue;
                }
                let tags = *tags.get_or_insert_with(|| tag(event));
                let scram = Scram {
                    number: *number,
                    docket: docket.as_str().to_owned(),
                    date: event.occurred.date,
                    kind,
                    initial_power: unit.initial_power,
                    tags,
                    episode: None,
                };
                scrams.push((scram, State::Waiting));
            }
        }
        scrams.sort_by(|(a, _), (b, _)| (a.date, a.number, &a.docket).cmp(&(b.date, b.number, &b.docket)));
        let mut by_docket: HashMap<String, Unit> = HashMap::new();
        for (index, (scram, _)) in scrams.iter().enumerate() {
            by_docket.entry(scram.docket.clone()).or_default().order.push(index);
        }
        Self {
            scrams,
            units: by_docket,
            names: units.all().iter().map(|unit| (unit.docket.as_str().to_owned(), unit.name.clone())).collect(),
            ..Self::default()
        }
    }

    /// Takes one date's rows.
    pub fn push(&mut self, days: &[UnitDay]) {
        for day in days {
            self.expose(day);
            let Some(unit) = self.units.get_mut(&day.docket) else { continue };
            Self::reach(&mut self.scrams, unit, day.date);
            Self::advance(&mut self.scrams, unit, day);
            Self::start(&mut self.scrams, unit, day);
        }
    }

    /// Counts `day` toward its unit's month.
    fn expose(&mut self, day: &UnitDay) {
        if !self.names.contains_key(&day.docket) {
            return;
        }
        let first = *self.first.get_or_insert(day.date.with_day(1).unwrap_or(day.date));
        self.last = self.last.max(Some(day.date));
        let Ok(index) = usize::try_from(months(first, day.date)) else { return };
        let days = self.days.entry(day.docket.clone()).or_default();
        if days.len() <= index {
            days.resize(index + 1, 0);
        }
        days[index] = days[index].saturating_add(1);
    }

    /// Moves the scrams dated up to `date` into waiting. One that can have an episode ends the
    /// running one; one from under [`DROP`]% power settles with none.
    fn reach(scrams: &mut [(Scram, State)], unit: &mut Unit, date: NaiveDate) {
        while let Some(&index) = unit.order.get(unit.next).filter(|&&index| scrams[index].0.date <= date) {
            unit.next += 1;
            let (scram, state) = &mut scrams[index];
            if scram.initial_power < DROP {
                *state = State::Done;
                continue;
            }
            if let Some(running) = unit.running.filter(|&running| scrams[running].0.date < scrams[index].0.date) {
                Self::close(&mut scrams[running], End::NextScram);
                unit.running = None;
            }
            unit.waiting.push(index);
        }
    }

    /// Counts `day` toward the running episode, or ends it: on a gap in the rows, a Refueling
    /// day, or a day back within [`DROP`] points.
    fn advance(scrams: &mut [(Scram, State)], unit: &mut Unit, day: &UnitDay) {
        let Some(running) = unit.running else { return };
        let (scram, state) = &mut scrams[running];
        let State::Running { episode, last } = state else { return };
        let end = if *last + Days::new(1) != day.date {
            Some(End::Open)
        } else if day.family == Family::Refueling {
            Some(End::Refueling)
        } else if !down(day.power, scram.initial_power) {
            Some(End::Returned)
        } else {
            None
        };
        if let Some(end) = end {
            Self::close(&mut scrams[running], end);
            unit.running = None;
        } else {
            episode.days += 1;
            episode.lost_kwh += lost_kwh(scram.initial_power, day);
            *last = day.date;
        }
    }

    /// Starts the episode of a waiting scram `day` shows the drop for, and settles those whose two
    /// days have passed without it.
    fn start(scrams: &mut [(Scram, State)], unit: &mut Unit, day: &UnitDay) {
        let mut waiting = std::mem::take(&mut unit.waiting);
        waiting.retain(|&index| {
            let (scram, state) = &mut scrams[index];
            if day.date > scram.date + Days::new(1) {
                *state = State::Done;
                return false;
            }
            if day.date < scram.date || !down(day.power, scram.initial_power) || unit.running.is_some() {
                return true;
            }
            let mut episode = Episode {
                start: day.date,
                days: 0,
                lost_kwh: 0,
                end: End::Refueling,
            };
            if day.family == Family::Refueling {
                scram.episode = Some(episode);
                *state = State::Done;
            } else {
                episode.days = 1;
                episode.lost_kwh = lost_kwh(scram.initial_power, day);
                *state = State::Running { episode, last: day.date };
                unit.running = Some(index);
            }
            false
        });
        unit.waiting = waiting;
    }

    /// Ends a running episode with `end`.
    fn close((scram, state): &mut (Scram, State), end: End) {
        if let State::Running { episode, .. } = *state {
            scram.episode = Some(Episode { end, ..episode });
        }
        *state = State::Done;
    }

    /// The series: every scram, by date, event number and docket, an episode still running
    /// being open, and each unit's days by month over the same months.
    #[must_use]
    pub fn finish(mut self) -> Series {
        for pair in &mut self.scrams {
            Self::close(pair, End::Open);
        }
        let count = match (self.first, self.last) {
            (Some(first), Some(last)) => usize::try_from(months(first, last) + 1).unwrap_or(0),
            _ => 0,
        };
        let units = self
            .days
            .into_iter()
            .map(|(docket, mut days)| {
                days.resize(count, 0);
                let name = self.names.get(&docket).cloned().unwrap_or_default();
                UnitExposure { docket, name, days }
            })
            .collect();
        Series {
            first: self.first,
            last: self.last,
            scrams: self.scrams.into_iter().map(|(scram, _)| scram).collect(),
            units,
        }
    }
}

/// Whole months from `from`'s month to `to`'s.
fn months(from: NaiveDate, to: NaiveDate) -> i64 {
    i64::from(to.year() - from.year()) * 12 + i64::from(to.month()) - i64::from(from.month())
}

/// The energy `day` lost below `initial` power, in kWh; nothing without a capacity.
fn lost_kwh(initial: u8, day: &UnitDay) -> u64 {
    day.capacity_kw
        .map_or(0, |capacity_kw| u64::from(initial.saturating_sub(day.power)) * u64::from(capacity_kw) * 24 / 100)
}
