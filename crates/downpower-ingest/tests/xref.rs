//! `See EN#` resolution (PRD-0003 uat-005) through the ledger builder, against the events on the
//! fixture pages in `fixtures/events`:
//!
//! - EN 45286, Crystal River 3, 2009-08-24: SCRAM Code `M/R`.
//! - EN 45290, Browns Ferry 3 alone, 2009-08-24: `M/R`.
//! - EN 35268, Catawba 2, 1999-01-15: `N`, 10 CFR 50.72(b)(1)(i)(A), a required shutdown.
//! - EN 35278, `FitzPatrick`, 1999-01-15: `N`, 50.72(b)(2)(iii)(D), which no rule names.
//! - EN 35269, Browns Ferry 2 and 3, 1999-01-15: retracted.
//! - EN 35275, Portsmouth Gaseous Diffusion Plant, 1999-01-15: a fuel cycle facility.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_core::barcode::Layer;
use downpower_ingest::attribution::{Evidence, Method};
use downpower_ingest::events::{self, Archive, EventSet, Scram};
use downpower_ingest::ledger::{Builder, Catalog, UnitDay, Unknowns};
use downpower_ingest::status::{Detail, Row, StatusPage};
use downpower_ingest::tags::Family;
use downpower_ingest::xref::{Decider, Reason, Resolved, Unresolved};

const PAGES: [&[u8]; 2] = [include_bytes!("fixtures/events/1999-01-15.html"), include_bytes!("fixtures/events/2009-08-24.html")];

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

/// The fixture pages' events, identified with the committed catalogs.
fn events(catalog: &Catalog) -> EventSet {
    let events: BTreeMap<u32, _> = PAGES.iter().flat_map(|page| events::parse(page).unwrap()).map(|event| (event.number, event)).collect();
    let identities = catalog.facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap();
    EventSet {
        events,
        identities,
        archive: Archive::default(),
    }
}

/// A status page with one row per `(unit, power, reason)`.
fn page(rows: &[(&str, u8, &str)]) -> StatusPage {
    StatusPage::Report(
        rows.iter()
            .map(|&(unit, power, reason)| Row {
                region: 2,
                unit: unit.to_owned(),
                power,
                detail: Some(Detail {
                    down: None,
                    reason: Some(reason.to_owned()),
                    changed: false,
                    scrams: 0,
                }),
            })
            .collect(),
    )
}

/// One day of `rows`, on a builder of its own.
fn one_day(date: &str, rows: &[(&str, u8, &str)]) -> Vec<UnitDay> {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog);
    Builder::new(&catalog, None, &events, Unknowns::Fail).day(day(date), Some(&page(rows))).unwrap()
}

/// `(cause or "-", family, method, evidence)` for compact comparisons.
fn label(unit_day: &UnitDay) -> (String, Family, Method, Evidence) {
    (unit_day.cause.clone().unwrap_or_else(|| "-".to_owned()), unit_day.family, unit_day.method, unit_day.evidence.clone())
}

fn resolved(number: u32, decided_by: Option<Decider>) -> Evidence {
    Evidence::Event(Resolved { number, decided_by })
}

fn unresolved(number: Option<u32>, reason: Reason) -> Evidence {
    Evidence::Xrefs(vec![Unresolved { number, reason }])
}

#[test]
fn uat_xref_resolution_a_scram_code_resolves_to_scram() {
    let days = one_day("2009-08-25", &[("Crystal River 3", 0, "See EN #45286")]);

    assert_eq!(
        label(&days[0]),
        ("scram".to_owned(), Family::Forced, Method::XrefEnV1, resolved(45286, Some(Decider::Scram(Scram::ManualReactor))))
    );
    // Attributed only: the reason itself names no cause.
    assert_eq!(days[0].cell(Layer::Observed).family, Family::Unattributed);
    assert_eq!(days[0].cell(Layer::Attributed).family, Family::Forced);
}

#[test]
fn uat_xref_resolution_a_required_shutdown_resolves_through_its_10_cfr_section() {
    let days = one_day("1999-01-16", &[("Catawba 2", 0, "SEE EN# 35268")]);

    assert_eq!(
        label(&days[0]),
        (
            "required_shutdown".to_owned(),
            Family::Forced,
            Method::XrefEnV1,
            resolved(35268, Some(Decider::Section("50.72(b)(1)(i)(A)".to_owned())))
        )
    );
}

#[test]
fn uat_xref_resolution_an_event_no_rule_matches_falls_back_to_en_reported() {
    let days = one_day("1999-01-16", &[("FitzPatrick", 50, "See EN #35278")]);

    assert_eq!(label(&days[0]), ("en_reported".to_owned(), Family::Forced, Method::XrefEnV1, resolved(35278, None)));
}

#[test]
fn uat_xref_resolution_stays_unresolved_for_a_missing_retracted_or_other_unit_event() {
    let days = one_day(
        "1999-01-16",
        &[
            ("Browns Ferry 2", 0, "SEE EN #35269"),
            ("Catawba 2", 0, "SEE EN #35275"),
            ("Cooper", 0, "SEE EN #99999"),
            ("FitzPatrick", 0, "SEE EN # PENDING"),
        ],
    );

    let by_docket: BTreeMap<&str, _> = days.iter().map(|unit_day| (unit_day.docket.as_str(), label(unit_day))).collect();
    let unattributed = |evidence| ("-".to_owned(), Family::Unattributed, Method::XrefUnresolved, evidence);
    assert_eq!(
        by_docket,
        BTreeMap::from([
            // Browns Ferry 2: listed by EN 35269, which the NRC retracted.
            ("05000260", unattributed(unresolved(Some(35269), Reason::Retracted))),
            ("05000298", unattributed(unresolved(Some(99999), Reason::Missing))),
            ("05000333", unattributed(unresolved(None, Reason::NoNumber))),
            // Catawba 2, citing a fuel cycle facility's event.
            ("05000414", unattributed(unresolved(Some(35275), Reason::OtherUnit))),
        ])
    );
}

#[test]
fn uat_xref_resolution_another_units_event_stays_unresolved() {
    let days = one_day("2009-08-25", &[("Browns Ferry 2", 0, "See EN #45290"), ("Browns Ferry 3", 0, "See EN #45290")]);

    assert_eq!(
        days.iter().map(label).collect::<Vec<_>>(),
        [
            ("-".to_owned(), Family::Unattributed, Method::XrefUnresolved, unresolved(Some(45290), Reason::OtherUnit)),
            ("scram".to_owned(), Family::Forced, Method::XrefEnV1, resolved(45290, Some(Decider::Scram(Scram::ManualReactor)))),
        ]
    );
}

#[test]
fn uat_xref_resolution_competes_in_precedence_and_loses_a_tie_to_the_reason() {
    let days = one_day(
        "2009-08-25",
        &[
            // Forced beats Maintenance.
            ("Crystal River 3", 40, "Reduced Power for Maintenance; See EN #45286"),
            // Both Forced: the cause the reason names wins.
            ("Browns Ferry 3", 0, "Forced Outage; See EN #45290"),
        ],
    );

    assert_eq!(
        days.iter().map(label).collect::<Vec<_>>(),
        [
            ("forced_outage".to_owned(), Family::Forced, Method::PrecedenceV1, Evidence::Tag("forced outage".to_owned())),
            ("scram".to_owned(), Family::Forced, Method::XrefEnV1, resolved(45286, Some(Decider::Scram(Scram::ManualReactor)))),
        ]
    );
}

#[test]
fn uat_xref_resolution_a_resolved_day_starts_an_inheritance_chain() {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog);
    let mut builder = Builder::new(&catalog, None, &events, Unknowns::Fail);

    builder.day(day("2009-08-25"), Some(&page(&[("Crystal River 3", 0, "See EN #45286")]))).unwrap();
    let startup = builder.day(day("2009-08-28"), Some(&page(&[("Crystal River 3", 20, "Startup in Progress")]))).unwrap();

    assert_eq!(label(&startup[0]), ("scram".to_owned(), Family::Forced, Method::Inherit14d, Evidence::InheritedFrom(day("2009-08-25"))));
}
