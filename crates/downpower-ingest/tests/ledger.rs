//! The ledger builder (PRD-0002 T-008) over the committed catalog and the `NUC_STATUS` fixture,
//! whose generators are Diablo Canyon 2 (2025-07-01 to 07-16) and Palisades (around its 2022-05-20
//! shutdown).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::attribution::{Evidence, Method};
use downpower_ingest::eia::NucStatus;
use downpower_ingest::ledger::{self, Builder, ObservedCause, Source, UnitDay, Unknowns};
use downpower_ingest::raw::{RawStore, Record, Series};
use downpower_ingest::status::{Detail, Row, StatusPage};
use downpower_ingest::tags::{Family, Tags};
use downpower_ingest::units::Units;

const NUC_STATUS: &[u8] = include_bytes!("fixtures/eia/nuc_status.txt");

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn row(unit: &str, power: u8, reason: &str) -> Row {
    Row {
        region: 4,
        unit: unit.to_owned(),
        power,
        detail: Some(Detail {
            down: None,
            reason: (!reason.is_empty()).then(|| reason.to_owned()),
            changed: false,
            scrams: 0,
        }),
    }
}

fn held(unit: &str, power: u8) -> Row {
    Row {
        region: 4,
        unit: unit.to_owned(),
        power,
        detail: None,
    }
}

fn page(rows: Vec<Row>) -> StatusPage {
    StatusPage::Report(rows)
}

struct Catalog {
    tags: Tags,
    units: Units,
    nuc_status: NucStatus,
}

fn catalog() -> Catalog {
    Catalog {
        tags: Tags::catalog().unwrap(),
        units: Units::catalog().unwrap(),
        nuc_status: NucStatus::parse(NUC_STATUS).unwrap(),
    }
}

impl Catalog {
    fn builder(&self, unknowns: Unknowns) -> Builder<'_> {
        Builder::new(&self.tags, &self.units, Some(&self.nuc_status), unknowns)
    }
}

#[test]
fn ledger_a_reported_day_carries_both_layers_and_eias_power() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);

    let days = builder
        .day(day("2025-07-05"), Some(&page(vec![row("Diablo Canyon 2", 55, "Reduced Power Due to Environmental Conditions")])))
        .unwrap();

    assert_eq!(days.len(), 1);
    let unit_day = &days[0];
    assert_eq!(
        (unit_day.docket.as_str(), unit_day.power, unit_day.source, unit_day.eia_power),
        ("05000323", 55, Source::NrcPage, Some(55))
    );
    assert_eq!(
        unit_day.causes,
        [ObservedCause {
            cause: "environmental_unspecified".to_owned(),
            tag: "reduced power due to environmental conditions".to_owned()
        }]
    );
    assert_eq!(
        (unit_day.cause.as_deref(), unit_day.family, unit_day.method),
        (Some("environmental_unspecified"), Family::Environmental, Method::PrecedenceV1)
    );
    let summary = builder.finish();
    assert_eq!((summary.compared, summary.disagreed, summary.days), (1, 0, 1));
}

#[test]
fn ledger_a_row_inside_the_hold_is_reason_pending() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);

    let days = builder.day(day("2025-07-05"), Some(&page(vec![held("Diablo Canyon 2", 55)]))).unwrap();

    assert_eq!((days[0].method, days[0].family, days[0].reason.as_deref()), (Method::ReasonPending, Family::Unattributed, None));
    assert_eq!(builder.finish().pending, 1);
}

#[test]
fn ledger_eia_fills_the_in_service_units_a_day_lacks() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);

    let no_data = builder.day(day("2025-07-05"), Some(&StatusPage::NoData)).unwrap();
    let no_page = builder.day(day("2025-07-06"), None).unwrap();

    assert_eq!(
        no_data,
        no_page
            .iter()
            .map(|unit_day| UnitDay {
                date: day("2025-07-05"),
                ..unit_day.clone()
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(no_data.len(), 1, "Palisades is out of service in 2025");
    assert_eq!((no_data[0].power, no_data[0].source, no_data[0].method), (55, Source::Eia, Method::NoReport));
    assert_eq!(builder.finish().from_eia, 2);
}

#[test]
fn ledger_days_after_a_shutdown_are_dropped() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);

    let last = builder.day(day("2022-05-20"), Some(&page(vec![row("Palisades", 97, "")]))).unwrap();
    let after = builder.day(day("2022-05-21"), Some(&page(vec![row("Palisades", 0, "Permanently Defueled")]))).unwrap();

    assert_eq!((last.len(), last[0].method, last[0].eia_power), (1, Method::NoCause, Some(97)));
    assert_eq!(after, []);
}

#[test]
fn ledger_rows_come_out_ordered_by_docket() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);

    let days = builder
        .day(
            day("2026-11-01"),
            Some(&page(vec![held("Wolf Creek 1", 100), held("Diablo Canyon 1", 100), held("Browns Ferry 1", 100)])),
        )
        .unwrap();

    let dockets: Vec<_> = days.iter().map(|unit_day| unit_day.docket.as_str()).collect();
    assert_eq!(dockets, ["05000259", "05000275", "05000482"]);
}

#[test]
fn ledger_inheritance_carries_across_days() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);

    builder.day(day("2025-07-01"), Some(&page(vec![row("Diablo Canyon 2", 0, "Refueling Outage")]))).unwrap();
    let days = builder.day(day("2025-07-03"), Some(&page(vec![row("Diablo Canyon 2", 30, "Increasing Power")]))).unwrap();

    assert_eq!(
        (days[0].cause.as_deref(), days[0].method, &days[0].evidence),
        (Some("refueling_outage"), Method::Inherit14d, &Evidence::InheritedFrom(day("2025-07-01")))
    );
}

#[test]
fn ledger_copies_that_differ_only_in_the_change_mark_merge() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);
    let mut marked = row("Wolf Creek 1", 100, "");
    marked.detail.as_mut().unwrap().changed = true;

    let days = builder.day(day("2006-06-22"), Some(&page(vec![row("Wolf Creek 1", 100, ""), marked]))).unwrap();

    assert_eq!((days.len(), days[0].changed), (1, true));
    assert_eq!(builder.finish().duplicates, 1);
}

#[test]
fn ledger_a_copy_repeating_the_previous_page_drops_out_in_either_order() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Fail);
    let stale = || row("LaSalle 1", 100, "Maximum Emergency Generation Alert");
    let fresh = || row("La Salle 1", 100, "");

    builder.day(day("2013-07-17"), Some(&page(vec![stale()]))).unwrap();
    let fresh_first = builder.day(day("2013-07-19"), Some(&page(vec![fresh(), stale()]))).unwrap();
    builder.day(day("2013-07-20"), Some(&page(vec![stale()]))).unwrap();
    let fresh_second = builder.day(day("2013-07-21"), Some(&page(vec![stale(), fresh()]))).unwrap();

    assert_eq!(fresh_first[0].flags, Vec::<String>::new());
    assert_eq!(fresh_second[0].flags, Vec::<String>::new());
}

#[test]
fn ledger_copies_no_rule_separates_fail_the_day() {
    let catalog = catalog();
    let mut builder = catalog.builder(Unknowns::Quarantine);

    let error = builder.day(day("2013-07-19"), Some(&page(vec![row("Perry 1", 100, ""), row("Perry 1", 96, "")]))).unwrap_err();

    assert!(error.to_string().contains("Perry 1"), "{error}");
}

#[test]
fn ledger_an_unknown_tag_fails_the_backfill_and_is_quarantined_on_the_server() {
    let catalog = catalog();
    let flux = || page(vec![row("Diablo Canyon 2", 80, "Zorbulating the flux capacitor; Refueling Outage")]);

    let error = catalog.builder(Unknowns::Fail).day(day("2025-07-05"), Some(&flux())).unwrap_err();
    let mut builder = catalog.builder(Unknowns::Quarantine);
    let days = builder.day(day("2025-07-05"), Some(&flux())).unwrap();

    assert!(error.to_string().contains("zorbulating the flux capacitor"), "{error}");
    assert_eq!((days[0].method, days[0].family), (Method::UnknownTag, Family::Unattributed));
    assert_eq!(days[0].evidence, Evidence::Unknown(vec!["zorbulating the flux capacitor".to_owned()]));
    assert_eq!(days[0].causes.len(), 1, "the known tags are still observed");
    assert_eq!(builder.finish().unknown_tags.into_iter().collect::<Vec<_>>(), [("zorbulating the flux capacitor".to_owned(), 1)]);
}

#[test]
fn ledger_an_unknown_unit_fails_the_backfill_and_is_skipped_on_the_server() {
    let catalog = catalog();
    let mystery = || page(vec![row("Mystery 9", 100, ""), row("Diablo Canyon 2", 100, "")]);

    let error = catalog.builder(Unknowns::Fail).day(day("2025-07-07"), Some(&mystery())).unwrap_err();
    let mut builder = catalog.builder(Unknowns::Quarantine);
    let days = builder.day(day("2025-07-07"), Some(&mystery())).unwrap();

    assert!(error.to_string().contains("Mystery 9"), "{error}");
    assert_eq!(days.len(), 1);
    assert_eq!(builder.finish().unknown_units.into_iter().collect::<Vec<_>>(), [("Mystery 9".to_owned(), 1)]);
}

#[test]
fn ledger_derived_version_is_a_stable_16_hex_digit_stamp() {
    let version = ledger::derived_version();

    assert_eq!(version.len(), 16);
    assert!(version.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(ledger::derived_version(), version);
}

/// A raw store holding the 2025-07-15 status page, a 404 for 07-16, and the `NUC_STATUS` fixture.
fn store(dir: &std::path::Path) -> RawStore {
    let store = RawStore::open(dir).unwrap();
    let at = "2026-11-14T00:00:00Z".parse().unwrap();
    let record = |series, date: &str, bytes: Option<&[u8]>| Record {
        source: downpower_ingest::raw::Source::Nrc,
        series,
        date: day(date),
        url: String::new(),
        status: if bytes.is_some() { 200 } else { 404 },
        captured_at: at,
        fetched_at: at,
        sha256: bytes.map(|bytes| store.put(bytes).unwrap()),
        len: bytes.map(|bytes| bytes.len() as u64),
    };
    store
        .append(&record(Series::PowerStatus, "2025-07-15", Some(include_bytes!("fixtures/status/2025-07-15.html"))))
        .unwrap();
    store.append(&record(Series::PowerStatus, "2025-07-16", None)).unwrap();
    store.append(&record(Series::NucStatus, "2026-11-14", Some(NUC_STATUS))).unwrap();
    store
}

#[test]
fn ledger_build_streams_from_the_first_page_to_the_last_day_either_source_covers() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let catalog = catalog();
    let mut days: Vec<UnitDay> = Vec::new();

    let summary = ledger::build(&store, &catalog.tags, &catalog.units, Unknowns::Fail, |rows| {
        days.extend(rows);
        Ok(())
    })
    .unwrap();

    let on_15th = days.iter().filter(|unit_day| unit_day.date == day("2025-07-15")).count();
    let on_16th: Vec<_> = days
        .iter()
        .filter(|unit_day| unit_day.date == day("2025-07-16"))
        .map(|unit_day| (unit_day.docket.as_str(), unit_day.source))
        .collect();
    assert!(on_15th > 90, "{on_15th} units on the 2025-07-15 page");
    assert_eq!(on_16th, [("05000323", Source::Eia)]);
    assert_eq!((summary.first, summary.last), (Some(day("2025-07-15")), Some(day("2025-07-16"))));
    assert_eq!(summary.days, days.len() as u64);
    assert_eq!((summary.compared, summary.disagreed), (1, 0));
}

#[test]
fn ledger_build_over_an_empty_store_builds_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();
    let catalog = catalog();

    let summary = ledger::build(&store, &catalog.tags, &catalog.units, Unknowns::Fail, |_| panic!("no rows")).unwrap();

    assert_eq!(summary, ledger::Summary::default());
}
