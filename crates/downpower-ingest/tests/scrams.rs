//! Both scram checks (PRD-0003 uat-007): the internal one through the ledger builder, and the
//! NRC's scram dataset (a committed copy, last modified 2025-05-13) reconciled by event number. Both
//! run against the events on the fixture page for 2009-08-24:
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
use downpower_ingest::raw::{RawStore, Record, Series, Source};
use downpower_ingest::scram_dataset::{self, Agreement, DatasetCounts, Row as DatasetRow};
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
        pages: BTreeMap::new(),
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

const DATASET: &[u8] = include_bytes!("fixtures/scrams/scram-event-notifications.xlsx");

/// An automatic scram's dataset row; `retracted` is its `NRC_retract_flag`.
fn row(event: u32, docket: &str, date: &str, retracted: bool) -> DatasetRow {
    DatasetRow {
        event,
        docket: docket.to_owned(),
        date: day(date),
        automatic: true,
        retracted,
    }
}

fn agreements(rows: &[DatasetRow], edit: impl FnOnce(&mut EventSet)) -> (Vec<(u32, String, Agreement)>, DatasetCounts) {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, edit);
    let reconciled = scram_dataset::reconcile(rows, &events).unwrap();
    let cases = reconciled.cases.iter().map(|case| (case.event, case.docket.clone(), case.outcome)).collect();
    (cases, reconciled.counts())
}

#[test]
fn uat_scram_check_the_nrc_dataset_parses_from_its_xlsx() {
    let rows = scram_dataset::parse(DATASET).unwrap();

    assert_eq!(rows.len(), 728);
    assert_eq!(rows[0], row(46556, "05000530", "2011-01-19", false));
    assert_eq!(rows.iter().filter(|row| row.retracted).count(), 9);
    assert_eq!(rows.iter().filter(|row| !row.automatic).count(), 290);
    assert_eq!(rows.iter().map(|row| row.date).min(), Some(day("2011-01-19")));
    assert_eq!(rows.iter().map(|row| row.date).max(), Some(day("2025-05-12")));
    assert!(rows.iter().any(|row| row.docket == "05200025"), "Vogtle 3 keeps its 052 docket");
}

/// The dataset with its `Docket` column renamed, as a format change would leave it.
fn renamed_column() -> Vec<u8> {
    use std::io::{Read as _, Write as _};
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(DATASET)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let name = file.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == "xl/sharedStrings.xml" {
            bytes = String::from_utf8(bytes).unwrap().replacen(">Docket<", ">Docket No<", 1).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn uat_scram_check_a_dataset_with_another_column_set_fails_to_parse() {
    assert!(scram_dataset::parse(b"not a workbook").is_err());
    let error = scram_dataset::parse(&renamed_column()).unwrap_err();
    assert!(format!("{error:#}").contains("Docket No"), "{error:#}");
}

#[test]
fn uat_scram_check_the_dataset_reconciles_by_event_and_docket() {
    let rows = [
        row(45286, "05000302", "2009-08-24", false),
        // Peach Bottom 2's line in EN 45289 has SCRAM Code `N`.
        row(45289, "05000277", "2009-08-24", false),
        row(45300, "05000296", "2009-08-24", true),
    ];
    let (cases, counts) = agreements(&rows, |_| {});

    assert_eq!(
        cases,
        [
            (45286, "05000302".to_owned(), Agreement::Agree),
            (45289, "05000277".to_owned(), Agreement::DatasetOnly),
            (45290, "05000296".to_owned(), Agreement::OursOnly),
            (45300, "05000296".to_owned(), Agreement::NrcRetracted),
        ]
    );
    assert_eq!((counts.agree, counts.dataset_only, counts.ours_only, counts.known), (1, 1, 1, 1));
    assert_eq!((counts.first, counts.last), (day("2009-01-01"), day("2009-08-24")));
}

#[test]
fn uat_scram_check_a_dataset_scram_we_hold_retracted_is_a_known_difference() {
    let retracted = |set: &mut EventSet| set.events.get_mut(&45290).unwrap().retracted = true;
    let (cases, counts) = agreements(&[row(45286, "05000302", "2009-08-24", false), row(45290, "05000296", "2009-08-24", false)], retracted);

    assert_eq!(cases, [(45286, "05000302".to_owned(), Agreement::Agree), (45290, "05000296".to_owned(), Agreement::WeRetracted)]);
    assert_eq!((counts.agree, counts.known), (1, 1));
}

#[test]
fn uat_scram_check_a_scram_the_nrc_flags_retracted_is_known_though_we_hold_it_live() {
    let (cases, counts) = agreements(&[row(45286, "05000302", "2009-08-24", true)], |_| {});

    assert_eq!(cases, [(45286, "05000302".to_owned(), Agreement::NrcRetracted), (45290, "05000296".to_owned(), Agreement::OursOnly)]);
    assert_eq!((counts.known, counts.ours_only), (1, 1));
}

#[test]
fn uat_scram_check_a_scram_from_a_reactor_not_critical_the_dataset_lacks_is_a_known_difference() {
    let shut_down = |set: &mut EventSet| set.events.get_mut(&45290).unwrap().units[0].critical = false;
    let (cases, counts) = agreements(&[row(45286, "05000302", "2009-08-24", false)], shut_down);

    assert_eq!(cases, [(45286, "05000302".to_owned(), Agreement::Agree), (45290, "05000296".to_owned(), Agreement::NotCritical)]);
    assert_eq!((counts.agree, counts.ours_only, counts.known), (1, 0, 1));
}

#[test]
fn uat_scram_check_our_scrams_outside_the_datasets_span_are_not_compared() {
    // The dataset ends the day before the fixture's scrams.
    let (cases, _) = agreements(&[row(45200, "05000296", "2009-08-23", false)], |_| {});

    assert_eq!(cases, [(45200, "05000296".to_owned(), Agreement::DatasetOnly)]);
}

/// A raw store holding the 2025-07-15 status page and `dataset` as the scram dataset.
fn store_with_dataset(dir: &std::path::Path, dataset: &[u8]) -> RawStore {
    let store = RawStore::open(dir).unwrap();
    let at = "2026-11-14T00:00:00Z".parse().unwrap();
    let record = |series, date: &str, bytes: &[u8]| Record {
        source: Source::Nrc,
        series,
        date: day(date),
        url: String::new(),
        status: 200,
        captured_at: at,
        fetched_at: at,
        sha256: Some(store.put(bytes).unwrap()),
        len: Some(bytes.len() as u64),
    };
    store.append(&record(Series::PowerStatus, "2025-07-15", include_bytes!("fixtures/status/2025-07-15.html"))).unwrap();
    store.append(&record(Series::ScramDataset, "2025-05-13", dataset)).unwrap();
    store
}

#[test]
fn uat_scram_check_the_build_reconciles_the_newest_stored_dataset() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store_with_dataset(tmp.path(), DATASET);
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, |_| {});

    let summary = downpower_ingest::ledger::build(&store, &catalog, &events, Unknowns::Fail, |_| Ok(())).unwrap();

    // The fixture's events are from 2009, before the dataset begins, so every row is the dataset's.
    let counts = summary.scrams.dataset.unwrap();
    assert_eq!((counts.agree, counts.dataset_only, counts.ours_only, counts.known), (0, 719, 0, 9));
    assert_eq!((counts.first, counts.last), (day("2011-01-01"), day("2025-05-12")));
}

#[test]
fn uat_scram_check_a_dataset_that_does_not_parse_fails_the_backfill_and_is_skipped_on_the_server() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store_with_dataset(tmp.path(), b"not a workbook");
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, |_| {});

    assert!(downpower_ingest::ledger::build(&store, &catalog, &events, Unknowns::Fail, |_| Ok(())).is_err());
    let summary = downpower_ingest::ledger::build(&store, &catalog, &events, Unknowns::Quarantine, |_| Ok(())).unwrap();
    assert_eq!(summary.scrams.dataset, None);
    assert!(summary.days > 0);
}
