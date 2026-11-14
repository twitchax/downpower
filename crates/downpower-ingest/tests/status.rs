//! The status-page parser against live NRC pages from every era (PRD-0002 uat-001). The fixtures are
//! public-domain pages copied byte for byte out of the raw store.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::status::{self, Detail, Row, StatusPage};

macro_rules! fixture {
    ($date:literal) => {
        include_bytes!(concat!("fixtures/status/", $date, ".html")).as_slice()
    };
}

/// Every settled fixture and its row count.
const SETTLED: [(&str, &[u8], usize); 6] = [
    ("1999-01-05", fixture!("1999-01-05"), 104),
    ("2008-07-15", fixture!("2008-07-15"), 104),
    ("2013-06-29", fixture!("2013-06-29"), 102),
    ("2016-07-15", fixture!("2016-07-15"), 100),
    ("2025-07-15", fixture!("2025-07-15"), 94),
    ("2026-10-15", fixture!("2026-10-15"), 95),
];

fn rows(page: &[u8]) -> Vec<Row> {
    match status::parse(page).unwrap() {
        StatusPage::Report(rows) => rows,
        StatusPage::NoData => panic!("expected rows, got NoData"),
    }
}

fn unit(rows: &[Row], name: &str) -> Row {
    rows.iter().find(|row| row.unit == name).cloned().unwrap_or_else(|| panic!("no {name}"))
}

fn row(region: u8, unit: &str, power: u8, down: Option<&str>, reason: Option<&str>, changed: bool, scrams: u8) -> Row {
    Row {
        region,
        unit: unit.to_owned(),
        power,
        detail: Some(Detail {
            down: down.map(|date| date.parse::<NaiveDate>().unwrap()),
            reason: reason.map(str::to_owned),
            changed,
            scrams,
        }),
    }
}

#[test]
fn uat_parse_eras_settled_pages_carry_all_six_columns() {
    for (date, page, count) in SETTLED {
        let rows = rows(page);

        assert_eq!(rows.len(), count, "{date}");
        assert!(rows.iter().all(|row| row.detail.is_some()), "{date}: a row without its detail");
        assert!(rows.is_sorted_by_key(|row| row.region), "{date}: regions out of order");
        assert!((1..=4).all(|region| rows.iter().any(|row| row.region == region)), "{date}: a region with no units");
        let reasons = rows.iter().filter_map(|row| row.detail.as_ref()?.reason.as_deref());
        for reason in reasons {
            assert!(!reason.contains(['\0', '\r']) && reason.trim() == reason, "{date}: {reason:?}");
        }
    }
}

#[test]
fn uat_parse_eras_1999_keeps_padding_out_and_non_ascii_in() {
    let rows = rows(fixture!("1999-01-05"));

    assert_eq!(
        unit(&rows, "Millstone 2"),
        row(1, "Millstone 2", 0, Some("1996-02-20"), Some("REFUELING OUTAGE - LOADING FUEL"), false, 0)
    );
    assert_eq!(unit(&rows, "FitzPatrick"), row(1, "FitzPatrick", 95, None, Some("Holding power level"), true, 0));
    assert_eq!(unit(&rows, "Indian Point 2"), row(1, "Indian Point 2", 99, None, None, false, 0));
    let palisades = unit(&rows, "Palisades").detail.unwrap().reason.unwrap();
    assert!(palisades.ends_with("CURRENT COLD LEG TEMP IS 413°F WITH A 20°F/HR HEATUP RATE"), "{palisades}");
}

#[test]
fn uat_parse_eras_2008_decodes_entities() {
    let rows = rows(fixture!("2008-07-15"));

    assert_eq!(
        unit(&rows, "FitzPatrick"),
        row(
            1,
            "FitzPatrick",
            55,
            None,
            Some(r#"REDUCTION IN POWER FOR PLANNED MAINTENANCE REPLACING SEALS ON "B" RX FEEDPUMP"#),
            false,
            0
        )
    );
}

#[test]
fn uat_parse_eras_2013_counts_scrams() {
    let rows = rows(fixture!("2013-06-29"));

    assert_eq!(
        unit(&rows, "Watts Bar 1"),
        row(
            2,
            "Watts Bar 1",
            0,
            Some("2013-06-28"),
            Some("FORCED OUTAGE - AUTOMATIC REACTOR TRIP DUE TO OFFSITE ELECTRICAL FAULT - SEE EN #49154"),
            false,
            1
        )
    );
    assert_eq!(rows.iter().map(|row| u32::from(row.detail.as_ref().unwrap().scrams)).sum::<u32>(), 1);
}

#[test]
fn uat_parse_eras_2016_matches_the_1999_template() {
    let rows = rows(fixture!("2016-07-15"));

    assert_eq!(
        unit(&rows, "Salem 1"),
        row(1, "Salem 1", 0, Some("2016-04-14"), Some("REFUELING OUTAGE - CORE RELOADED - RV HEAD SET BUT NOT TENSIONED"), true, 0)
    );
}

#[test]
fn uat_parse_eras_2025_strips_nuls_and_crlfs() {
    let rows = rows(fixture!("2025-07-15"));

    assert_eq!(unit(&rows, "FitzPatrick"), row(1, "FitzPatrick", 100, None, None, false, 0));
    assert_eq!(
        unit(&rows, "Beaver Valley 2"),
        row(1, "Beaver Valley 2", 99, None, Some("Steam Generator Limitations;\n\nMaximum Generation Alert"), true, 0)
    );
}

#[test]
fn uat_parse_eras_2026_settled_page_has_its_reasons() {
    let rows = rows(fixture!("2026-10-15"));

    assert_eq!(unit(&rows, "Robinson 2"), row(2, "Robinson 2", 0, Some("2026-10-03"), Some("Refueling Outage"), true, 0));
}

#[test]
fn uat_parse_eras_a_page_inside_the_hold_is_numbers_only() {
    let rows = rows(fixture!("2026-11-12"));

    assert_eq!(rows.len(), 95);
    assert!(rows.iter().all(|row| row.detail.is_none()));
    assert_eq!(
        unit(&rows, "Robinson 2"),
        Row {
            region: 2,
            unit: "Robinson 2".to_owned(),
            power: 17,
            detail: None
        }
    );
}

#[test]
fn uat_parse_eras_a_missing_day_is_no_data() {
    assert_eq!(status::parse(fixture!("1999-05-08")).unwrap(), StatusPage::NoData);
}
