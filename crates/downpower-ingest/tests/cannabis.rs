//! The legalization catalog (PRD-0005 uat-003): every state and DC, dates that parse and are in
//! order, a source for every row, and the attribution notice its Wikipedia source requires.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::cannabis::Laws;

const CATALOG: &str = include_str!("../../../catalog/cannabis.csv");

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

#[test]
fn uat_cannabis_catalog_covers_every_state_and_dc_in_order() {
    let laws = Laws::catalog().unwrap();
    assert_eq!(laws.iter().count(), 51);
    assert!(laws.get("DC").is_some());
    for law in laws.iter() {
        assert!(!law.source.is_empty(), "{}", law.state);
        if let (Some(adult_use), Some(sales)) = (law.adult_use, law.sales) {
            assert!(adult_use <= sales, "{}", law.state);
        }
    }
    let colorado = laws.get("CO").unwrap();
    assert!(!colorado.adult_use_on(day("2012-12-09")) && colorado.adult_use_on(day("2012-12-10")));
    assert!(colorado.medical_on(day("2001-01-01")));
    assert!(!laws.get("ID").unwrap().medical_on(day("2026-11-18")));
}

#[test]
fn uat_cannabis_catalog_carries_its_attribution_notice() {
    let header: String = CATALOG.lines().take_while(|line| line.starts_with('#')).collect();
    for needed in ["Wikipedia", "CC BY-SA 4.0", "https://creativecommons.org/licenses/by-sa/4.0/"] {
        assert!(header.contains(needed), "{needed}");
    }
}

#[test]
fn uat_cannabis_catalog_rejects_a_bad_row() {
    let head = "state,adult_use,sales,medical,source\n";
    assert!(Laws::parse(&format!("{head}CO,2014-01-01,2012-12-10,,https://example.org\n")).is_err(), "sales before adult use");
    assert!(Laws::parse(&format!("{head}CO,,2014-01-01,,https://example.org\n")).is_err(), "sales without adult use");
    assert!(Laws::parse(&format!("{head}CO,,,,\n")).is_err(), "no source");
    assert!(Laws::parse(&format!("{head}CO,2012-12-1O,,,https://example.org\n")).is_err(), "a date that doesn't parse");
    assert!(Laws::parse(&format!("{head}CO,,,,https://example.org\nCO,,,,https://example.org\n")).is_err(), "twice");
    assert!(Laws::parse(&format!("# a note\n{head}co,,,,https://example.org\n")).is_err(), "lower case");
}
