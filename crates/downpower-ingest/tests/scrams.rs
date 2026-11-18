//! The internal scram check (PRD-0003 uat-007, its internal half) through the ledger builder,
//! against the events on the fixture page for 2009-08-24:
//!
//! - EN 45286, Crystal River 3: SCRAM Code `M/R` from 100% at 11:00.
//! - EN 45290, Browns Ferry 3: `M/R` from 100% at 18:50.
//! - EN 45289, Peach Bottom 2 and 3: `N`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::events::{self, Archive, EventSet};
use downpower_ingest::ledger::{Builder, Catalog, Unknowns};
use downpower_ingest::scrams::{Check, EventCounts, Found, ReportCounts, Shown};
use downpower_ingest::status::{Detail, Row, StatusPage};

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
        archive: Archive::default(),
    };
    edit(&mut set);
    set
}

/// One unit's status row: power, reason, the Scrams (#) count and the Down date.
#[derive(Clone, Copy)]
struct Line {
    unit: &'static str,
    power: u8,
    reason: Option<&'static str>,
    scrams: u8,
    down: Option<&'static str>,
}

const fn line(unit: &'static str, power: u8) -> Line {
    Line {
        unit,
        power,
        reason: None,
        scrams: 0,
        down: None,
    }
}

impl Line {
    const fn reason(self, reason: &'static str) -> Self {
        Self { reason: Some(reason), ..self }
    }

    const fn flagged(self, down: &'static str) -> Self {
        Self { scrams: 1, down: Some(down), ..self }
    }
}

fn page(lines: &[Line]) -> StatusPage {
    StatusPage::Report(
        lines
            .iter()
            .map(|line| Row {
                region: 2,
                unit: line.unit.to_owned(),
                power: line.power,
                detail: Some(Detail {
                    down: line.down.map(day),
                    reason: line.reason.map(str::to_owned),
                    changed: false,
                    scrams: line.scrams,
                }),
            })
            .collect(),
    )
}

/// Builds `pages` (one per day from 2009-08-23) over the fixture events after `edit`, and returns
/// the check's cases and the build summary's counts.
fn check(edit: impl FnOnce(&mut EventSet), pages: &[&[Line]]) -> (downpower_ingest::scrams::Cases, EventCounts, ReportCounts) {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, edit);
    let mut builder = Builder::new(&catalog, None, &events, Unknowns::Fail);
    let mut check = Check::new(&events);
    for (date, lines) in day("2009-08-23").iter_days().zip(pages) {
        check.observe(&builder.day(date, Some(&page(lines))).unwrap());
    }
    let summary = builder.finish();
    (check.finish(), summary.scrams.events, summary.scrams.reports)
}

fn shown(cases: &downpower_ingest::scrams::Cases) -> Vec<(u32, Shown, [Option<u8>; 2])> {
    cases.events.iter().map(|case| (case.number, case.outcome, case.powers)).collect()
}

fn found(cases: &downpower_ingest::scrams::Cases) -> Vec<(&str, NaiveDate, Found, Vec<u32>)> {
    cases.reports.iter().map(|case| (case.docket.as_str(), case.date, case.outcome, case.events.clone())).collect()
}

const CR3: &str = "Crystal River 3";
const BF3: &str = "Browns Ferry 3";

#[test]
fn uat_scram_check_an_en_scram_shows_as_a_drop_or_is_named() {
    let (cases, events, _) = check(
        |_| {},
        &[
            &[line(CR3, 100), line(BF3, 100)],
            // Both reports come before their scrams.
            &[line(CR3, 100), line(BF3, 100)],
            // Crystal River is down; Browns Ferry is back at 95% and cites the event.
            &[line(CR3, 0), line(BF3, 95).reason("See EN #45290")],
        ],
    );

    assert_eq!(shown(&cases), [(45286, Shown::Dropped, [Some(100), Some(0)]), (45290, Shown::Named, [Some(100), Some(95)])]);
    assert_eq!(
        events,
        EventCounts {
            dropped: 1,
            named: 1,
            ..EventCounts::default()
        }
    );
    assert_eq!((events.agreed(), events.checked()), (2, 2));
}

#[test]
fn uat_scram_check_an_en_scram_the_reports_miss_or_cannot_show() {
    let low = |set: &mut EventSet| set.events.get_mut(&45290).unwrap().units[0].initial_power = 9;
    let flat = [line(CR3, 100), line(BF3, 9)];
    let (cases, events, _) = check(low, &[&flat, &flat, &flat]);

    assert_eq!(shown(&cases), [(45286, Shown::Missed, [Some(100), Some(100)]), (45290, Shown::LowPower, [Some(9), Some(9)])]);
    assert_eq!((events.agreed(), events.checked()), (0, 1));

    // No report lists Browns Ferry 3, and a retracted event isn't checked at all.
    let retracted = |set: &mut EventSet| set.events.get_mut(&45286).unwrap().retracted = true;
    let (cases, _, _) = check(retracted, &[&[line(CR3, 100)], &[line(CR3, 100)], &[line(CR3, 0)]]);
    assert_eq!(shown(&cases), [(45290, Shown::NoReport, [None, None])]);
}

#[test]
fn uat_scram_check_a_status_report_scram_finds_its_en() {
    let (cases, _, reports) = check(
        |_| {},
        &[
            &[line(CR3, 100), line(BF3, 100), line("Browns Ferry 2", 100), line("Peach Bottom 2", 100)],
            &[line(CR3, 100), line(BF3, 100), line("Browns Ferry 2", 100), line("Peach Bottom 2", 100)],
            &[
                // A reason naming a scram, and the Scrams (#) count with the Down date.
                line(CR3, 0).reason("Automatic Reactor Trip"),
                line(BF3, 0).flagged("2009-08-24"),
                // No event on Browns Ferry 2, and Peach Bottom 2's says `N`.
                line("Browns Ferry 2", 0).reason("Reactor Scram"),
                line("Peach Bottom 2", 0).reason("Manual Reactor Scram"),
            ],
            // A scram marked on consecutive reports is one scram.
            &[line(CR3, 0).reason("Automatic Reactor Trip")],
        ],
    );

    assert_eq!(
        found(&cases),
        [
            ("05000260", day("2009-08-25"), Found::Missed, vec![]),
            ("05000277", day("2009-08-25"), Found::NoScramCode, vec![45289]),
            ("05000296", day("2009-08-24"), Found::Event, vec![45290]),
            ("05000302", day("2009-08-25"), Found::Event, vec![45286]),
        ]
    );
    assert_eq!(
        reports,
        ReportCounts {
            event: 2,
            no_scram_code: 1,
            missed: 1,
            ..ReportCounts::default()
        }
    );
    assert_eq!((reports.agreed(), reports.checked()), (2, 4));
}

#[test]
fn uat_scram_check_a_report_scram_whose_event_was_retracted_is_a_known_difference() {
    let retracted = |set: &mut EventSet| set.events.get_mut(&45286).unwrap().retracted = true;
    let (cases, _, reports) = check(retracted, &[&[line(CR3, 100)], &[line(CR3, 100)], &[line(CR3, 0).reason("Manual Reactor Trip")]]);

    assert_eq!(found(&cases), [("05000302", day("2009-08-25"), Found::Retracted, vec![45286])]);
    assert_eq!((reports.retracted, reports.agreed(), reports.checked()), (1, 0, 0));
}

#[test]
fn uat_scram_check_a_report_citing_its_event_finds_it_outside_the_window() {
    // The first report marking the scram comes nine days on, citing it.
    let mut pages: Vec<Vec<Line>> = vec![vec![line(CR3, 100)]; 10];
    pages.push(vec![line(CR3, 0).reason("Reactor Trip; See EN #45286")]);
    let pages: Vec<&[Line]> = pages.iter().map(Vec::as_slice).collect();
    let (cases, _, _) = check(|_| {}, &pages);

    assert_eq!(found(&cases), [("05000302", day("2009-09-02"), Found::Event, vec![45286])]);
}

#[test]
fn uat_scram_check_a_citation_counts_inside_a_tag_with_another_label() {
    // The scram tag itself cites the event, nine days on, so no `xref` tag carries the number.
    let mut pages: Vec<Vec<Line>> = vec![vec![line(CR3, 100)]; 10];
    pages.push(vec![line(CR3, 0).reason("Reactor Trip - See EN #45286")]);
    let pages: Vec<&[Line]> = pages.iter().map(Vec::as_slice).collect();
    let (cases, _, _) = check(|_| {}, &pages);

    assert_eq!(found(&cases), [("05000302", day("2009-09-02"), Found::Event, vec![45286])]);

    // Browns Ferry 3 shows no drop, but its forced outage tag cites the event.
    let (cases, _, _) = check(|_| {}, &[&[line(BF3, 100)], &[line(BF3, 100)], &[line(BF3, 95).reason("Forced outage - see EN #45290")]]);

    assert_eq!(shown(&cases), [(45286, Shown::NoReport, [None, None]), (45290, Shown::Named, [Some(100), Some(95)])]);
}
