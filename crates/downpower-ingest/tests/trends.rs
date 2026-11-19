//! The fleet trends a rebuild collects (PRD-0006 uat-001), through the ledger builder, over eight
//! status pages from 2009-08-23 and the events on the fixture page for 2009-08-24:
//!
//! - EN 45286, Crystal River 3: SCRAM Code `M/R`.
//! - EN 45290, Browns Ferry 3: `M/R`.
//! - EN 45289, Peach Bottom 2 and 3: `N`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_core::trends::{Month, Outage, Trends};
use downpower_ingest::events::{self, Archive, EventSet, Scram};
use downpower_ingest::ledger::{Builder, Catalog, Unknowns};
use downpower_ingest::status::{Detail, Row, StatusPage};
use downpower_ingest::trends::Collector;

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

/// The fixture page's events, identified with the committed catalogs, after `edit`.
fn events(catalog: &Catalog, edit: impl FnOnce(&mut EventSet)) -> EventSet {
    let events: BTreeMap<u32, _> = events::parse(include_bytes!("fixtures/events/2009-08-24.html"))
        .unwrap()
        .into_iter()
        .map(|event| (event.number, event))
        .collect();
    let identities = catalog.facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap();
    let mut set = EventSet {
        events,
        identities,
        pages: BTreeMap::new(),
        archive: Archive::default(),
    };
    edit(&mut set);
    set
}

const CR3: &str = "Crystal River 3";
const BF3: &str = "Browns Ferry 3";
const BF2: &str = "Browns Ferry 2";
const PB2: &str = "Peach Bottom 2";
const PB3: &str = "Peach Bottom 3";

const REFUELING: &str = "Refueling Outage";
const FORCED: &str = "Forced Outage";

/// A unit's power and reason on one page; `None` leaves the unit off it.
type Line = Option<(u8, Option<&'static str>)>;

/// Each unit's lines per day from 2009-08-23.
fn lines() -> Vec<(&'static str, [Line; 8])> {
    let up = Some((100, None));
    let refueling = Some((0, Some(REFUELING)));
    let forced = Some((0, Some(FORCED)));
    vec![
        // Bracketed by days at power, all Refueling: an outage of three days.
        (CR3, [up, refueling, refueling, refueling, Some((20, Some("Increasing Power"))), up, up, up]),
        // At 0% on the first page: nothing before it shows when it began.
        (BF3, [refueling, refueling, up, up, up, up, up, up]),
        // Mostly Forced.
        (BF2, [up, forced, refueling, forced, up, up, up, up]),
        // Still down on the last page.
        (PB2, [up, up, up, up, up, refueling, refueling, refueling]),
        // Off one page mid-run, which breaks the run.
        (PB3, [up, refueling, None, refueling, up, up, up, up]),
    ]
}

fn page(date: usize) -> StatusPage {
    StatusPage::Report(
        lines()
            .into_iter()
            .filter_map(|(unit, days)| {
                let (power, reason) = days[date]?;
                Some(Row {
                    region: 2,
                    unit: unit.to_owned(),
                    power,
                    detail: Some(Detail {
                        down: None,
                        reason: reason.map(str::to_owned),
                        changed: false,
                        scrams: 0,
                    }),
                })
            })
            .collect(),
    )
}

/// The trends from the eight pages over the fixture events after `edit`, every row given a
/// capacity of 1 MW.
fn collect(edit: impl FnOnce(&mut EventSet)) -> Trends {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, edit);
    let mut builder = Builder::new(&catalog, None, &events, Unknowns::Fail);
    let mut collector = Collector::new(&events, &catalog.units);
    for (index, date) in day("2009-08-23").iter_days().take(8).enumerate() {
        let mut days = builder.day(date, Some(&page(index))).unwrap();
        for row in &mut days {
            row.capacity_kw = Some(1_000);
        }
        collector.push(&days).unwrap();
    }
    collector.finish()
}

#[test]
fn uat_trends_collect_counts_exposure_capacity_and_scrams() {
    let trends = collect(|_| {});
    assert_eq!(trends.first, Some(day("2009-08-01")));
    assert_eq!(trends.last, Some(day("2009-08-30")));
    // Five units on every page but one, where Peach Bottom 3 is missing.
    assert_eq!(
        trends.months,
        [Month {
            unit_days: 39,
            capacity_kwh: 39 * 24_000,
            scrams: [0, 2],
        }]
    );
}

#[test]
fn uat_trends_collect_takes_scrams_by_kind_and_skips_retracted_events() {
    let trends = collect(|set| {
        set.events.get_mut(&45290).unwrap().units[0].scram = Some(Scram::AutoReactor);
        set.events.get_mut(&45286).unwrap().retracted = true;
    });
    assert_eq!(trends.months[0].scrams, [1, 0]);
}

#[test]
fn uat_trends_collect_finds_only_bracketed_refueling_outages() {
    let trends = collect(|_| {});
    assert_eq!(
        trends.outages,
        [Outage {
            docket: "05000302".to_owned(),
            start: day("2009-08-24"),
            days: 3,
        }]
    );
}
