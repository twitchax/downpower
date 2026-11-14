//! The committed unit spine against every name the status pages have used and every generator in
//! EIA's `NUC_STATUS` (PRD-0002 uat-006). Both lists are drawn from the 2026-11-13 backfill and the
//! `NUC_STATUS` file of the same day.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use pretty_assertions::assert_eq;

use downpower_ingest::status::{self, StatusPage};
use downpower_ingest::units::{EiaGenerator, Units};

/// Every unit name on any status page from 1999-01-01 to 2026-11-13.
const UNIT_NAMES: &str = include_str!("fixtures/status/unit-names.txt");
/// Every generator series in `NUC_STATUS`, `NUC_STATUS.CAP.{plant}-{generator}.D` and its plant name.
const GENERATORS: &str = include_str!("fixtures/eia/nuc_status_generators.tsv");

fn generators() -> Vec<EiaGenerator> {
    GENERATORS
        .lines()
        .map(|line| {
            let id = line.split('\t').next().unwrap().trim_start_matches("NUC_STATUS.CAP.").trim_end_matches(".D");
            let (plant, generator) = id.split_once('-').unwrap();
            EiaGenerator {
                plant: plant.parse().unwrap(),
                generator: generator.to_owned(),
            }
        })
        .collect()
}

#[test]
fn uat_crosswalk_every_page_name_maps_and_every_catalog_name_was_used() {
    let units = Units::catalog().unwrap();
    let seen: BTreeSet<&str> = UNIT_NAMES.lines().collect();

    let unknown: Vec<_> = seen.iter().filter(|name| units.by_nrc_name(name).is_none()).collect();
    let unused: Vec<_> = units.all().iter().flat_map(|unit| &unit.nrc_names).filter(|name| !seen.contains(name.as_str())).collect();

    assert_eq!(unknown, Vec::<&&str>::new());
    assert_eq!(unused, Vec::<&String>::new());
    assert_eq!(units.all().len(), 107);
}

#[test]
fn uat_crosswalk_every_generator_maps_one_to_one() {
    let units = Units::catalog().unwrap();
    let generators = generators();

    let unmapped: Vec<_> = generators.iter().filter(|generator| units.by_eia(generator).is_none()).collect();
    let catalog: BTreeSet<_> = units.all().iter().map(|unit| (unit.eia.plant, unit.eia.generator.as_str())).collect();
    let eia: BTreeSet<_> = generators.iter().map(|generator| (generator.plant, generator.generator.as_str())).collect();

    assert_eq!(unmapped, Vec::<&EiaGenerator>::new());
    assert_eq!(catalog, eia);
}

#[test]
fn uat_crosswalk_every_fixture_row_maps() {
    let units = Units::catalog().unwrap();
    for path in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/status")).unwrap() {
        let path = path.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "html") {
            continue;
        }
        let StatusPage::Report(rows) = status::parse(&std::fs::read(&path).unwrap()).unwrap() else {
            continue;
        };
        for row in rows {
            assert!(units.by_nrc_name(&row.unit).is_some(), "{}: {}", path.display(), row.unit);
        }
    }
}

#[test]
fn uat_crosswalk_an_unknown_name_has_no_unit() {
    assert_eq!(Units::catalog().unwrap().by_nrc_name("Zion 1"), None);
}

#[test]
fn shutdowns_match_the_last_day_at_power_on_the_pages() {
    let units = Units::catalog().unwrap();
    let retired: Vec<_> = units.all().iter().filter(|unit| unit.shutdown.is_some()).map(|unit| unit.name.as_str()).collect();

    assert_eq!(
        retired,
        [
            "Crystal River 3",
            "Duane Arnold",
            "Fort Calhoun",
            "Indian Point 2",
            "Indian Point 3",
            "Kewaunee",
            "Oyster Creek",
            "Palisades",
            "Pilgrim 1",
            "San Onofre 2",
            "San Onofre 3",
            "Three Mile Island 1",
            "Vermont Yankee"
        ]
    );
    assert!(units.all().iter().all(|unit| unit.restart.is_none()));
}
