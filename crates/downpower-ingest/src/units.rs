//! The unit spine (DESIGN.md Decision 16): every reactor unit on the status pages since 1999, keyed by
//! its NRC docket, from the committed `catalog/units.csv`. A name the catalog doesn't know fails the
//! backfill, so a new spelling is added on purpose.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

use anyhow::{Context as _, bail, ensure};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::Res;

/// The catalog, embedded so the binary carries it.
pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/units.csv"));

/// An NRC docket number, e.g. `05000275` (Diablo Canyon 1). Part 50 licenses are `05000xxx` and
/// Part 52 licenses (Vogtle 3 and 4) are `05200xxx`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Docket(String);

impl Docket {
    /// The eight digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Docket {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Res<Self> {
        let digits = value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit());
        ensure!(digits && (value.starts_with("05000") || value.starts_with("05200")), "not an NRC power reactor docket: {value:?}");
        Ok(Self(value))
    }
}

impl fmt::Display for Docket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An EIA generator, as `NUC_STATUS` series ids name it (`NUC_STATUS.OUT.6099-1.D` is plant 6099,
/// generator 1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EiaGenerator {
    pub plant: u32,
    pub generator: String,
}

/// One reactor unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub docket: Docket,
    /// The name the site shows.
    pub name: String,
    /// Every spelling the status pages have used for the unit.
    pub nrc_names: Vec<String>,
    pub eia: EiaGenerator,
    /// The first day of commercial operation, for a unit that began after 1999. The pages list a
    /// new unit through months of startup testing at low power, which is no lost output.
    pub commercial: Option<NaiveDate>,
    /// The last day in service before the unit stopped operating, for good or, with a restart, for
    /// an extended shutdown. The morning report usually still shows the unit at power that day.
    pub shutdown: Option<NaiveDate>,
    /// The day a shut-down unit returned to service.
    pub restart: Option<NaiveDate>,
    /// Where the commercial, shutdown and restart dates come from.
    pub source: Option<String>,
}

impl Unit {
    /// Whether the unit was in service on `date`: in commercial operation, and not after its
    /// shutdown unless it has since restarted. A unit out of service loses no output, whatever its
    /// Power column reads.
    #[must_use]
    pub fn in_service(&self, date: NaiveDate) -> bool {
        if self.commercial.is_some_and(|commercial| date < commercial) {
            return false;
        }
        match (self.shutdown, self.restart) {
            (Some(shutdown), Some(restart)) => date <= shutdown || date >= restart,
            (Some(shutdown), None) => date <= shutdown,
            (None, _) => true,
        }
    }
}

/// A row of `catalog/units.csv`. `nrc_names` holds every spelling, joined by `;`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvRow {
    docket: String,
    name: String,
    nrc_names: String,
    eia_plant: u32,
    eia_generator: String,
    commercial: Option<NaiveDate>,
    shutdown: Option<NaiveDate>,
    restart: Option<NaiveDate>,
    source: Option<String>,
}

/// The catalog, indexed by every key a source uses.
#[derive(Debug, Clone)]
pub struct Units {
    all: Vec<Unit>,
    by_nrc_name: HashMap<String, usize>,
    by_eia: HashMap<EiaGenerator, usize>,
}

impl Units {
    /// The committed catalog.
    ///
    /// # Errors
    ///
    /// When `catalog/units.csv` breaks a rule [`Units::parse`] checks.
    pub fn catalog() -> Res<Self> {
        Self::parse(CATALOG).context("catalog/units.csv")
    }

    /// Parses a units table.
    ///
    /// # Errors
    ///
    /// When a row doesn't parse, a docket is malformed, a docket, NRC name or EIA generator appears
    /// twice, a shutdown has no source, or a restart doesn't follow its shutdown.
    pub fn parse(csv: &str) -> Res<Self> {
        let mut units = Vec::new();
        let mut dockets = HashMap::new();
        let mut by_nrc_name = HashMap::new();
        let mut by_eia = HashMap::new();
        for (index, row) in csv::Reader::from_reader(csv.as_bytes()).deserialize::<CsvRow>().enumerate() {
            let line = index + 2;
            let row = row.with_context(|| format!("line {line}"))?;
            let unit = Unit {
                docket: Docket::try_from(row.docket).with_context(|| format!("line {line}"))?,
                nrc_names: row.nrc_names.split(';').map(str::to_owned).collect(),
                name: row.name,
                eia: EiaGenerator {
                    plant: row.eia_plant,
                    generator: row.eia_generator,
                },
                commercial: row.commercial,
                shutdown: row.shutdown,
                restart: row.restart,
                source: row.source,
            };
            ensure!(
                (unit.commercial.is_none() && unit.shutdown.is_none()) || unit.source.is_some(),
                "line {line}: {} has a date but no source",
                unit.name
            );
            if let (Some(commercial), Some(shutdown)) = (unit.commercial, unit.shutdown) {
                ensure!(commercial < shutdown, "line {line}: {} shuts down before commercial operation", unit.name);
            }
            if let Some(restart) = unit.restart {
                ensure!(
                    unit.shutdown.is_some_and(|shutdown| shutdown < restart),
                    "line {line}: {} restarts without an earlier shutdown",
                    unit.name
                );
            }
            let slot = units.len();
            ensure!(dockets.insert(unit.docket.clone(), slot).is_none(), "line {line}: docket {} appears twice", unit.docket);
            for name in &unit.nrc_names {
                ensure!(!name.is_empty() && name.trim() == name, "line {line}: NRC name {name:?} is blank or padded");
                insert_unique(&mut by_nrc_name, name.clone(), slot).with_context(|| format!("line {line}: NRC name {name:?}"))?;
            }
            insert_unique(&mut by_eia, unit.eia.clone(), slot).with_context(|| format!("line {line}: EIA generator {:?}", unit.eia))?;
            units.push(unit);
        }
        Ok(Self { all: units, by_nrc_name, by_eia })
    }

    /// Every unit, in catalog order.
    #[must_use]
    pub fn all(&self) -> &[Unit] {
        &self.all
    }

    /// The unit a status page calls `name`.
    #[must_use]
    pub fn by_nrc_name(&self, name: &str) -> Option<&Unit> {
        self.by_nrc_name.get(name).map(|&index| &self.all[index])
    }

    /// The unit behind an EIA generator.
    #[must_use]
    pub fn by_eia(&self, generator: &EiaGenerator) -> Option<&Unit> {
        self.by_eia.get(generator).map(|&index| &self.all[index])
    }
}

fn insert_unique<K: std::hash::Hash + Eq>(map: &mut HashMap<K, usize>, key: K, slot: usize) -> Res<()> {
    match map.entry(key) {
        Entry::Occupied(_) => bail!("appears twice"),
        Entry::Vacant(entry) => {
            entry.insert(slot);
            Ok(())
        }
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    const HEADER: &str = "docket,name,nrc_names,eia_plant,eia_generator,commercial,shutdown,restart,source\n";

    fn error(rows: &str) -> String {
        format!("{:#}", Units::parse(&format!("{HEADER}{rows}")).unwrap_err())
    }

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn every_key_finds_its_unit() {
        let units = Units::parse(&format!("{HEADER}05000373,LaSalle 1,LaSalle 1;La Salle 1,6026,1,,,,\n")).unwrap();
        let unit = &units.all()[0];

        assert_eq!(unit.docket.as_str(), "05000373");
        assert_eq!(units.by_nrc_name("La Salle 1"), Some(unit));
        assert_eq!(units.by_nrc_name("LaSalle 1"), Some(unit));
        assert_eq!(units.by_eia(&EiaGenerator { plant: 6026, generator: "1".to_owned() }), Some(unit));
        assert_eq!(units.by_nrc_name("Zion 1"), None);
    }

    #[test]
    fn service_ends_after_the_shutdown_day_and_resumes_at_restart() {
        let units = Units::parse(&format!(
            "{HEADER}05000255,Palisades,Palisades,1715,1,,2022-05-20,2025-10-01,test\n05000271,Vermont Yankee,Vermont Yankee,3751,1,,2014-12-29,,test\n05200025,Vogtle 3,Vogtle 3,649,3,2023-07-31,,,test\n"
        ))
        .unwrap();
        let [palisades, vermont, vogtle] = units.all() else { panic!("three units") };

        assert!(palisades.in_service(day("2022-05-20")));
        assert!(!palisades.in_service(day("2022-05-21")));
        assert!(!palisades.in_service(day("2025-09-30")));
        assert!(palisades.in_service(day("2025-10-01")));
        assert!(vermont.in_service(day("2014-12-29")));
        assert!(!vermont.in_service(day("2014-12-30")));
        assert!(!vogtle.in_service(day("2023-07-30")));
        assert!(vogtle.in_service(day("2023-07-31")));
    }

    #[test]
    fn the_table_is_checked() {
        let cases = [
            ("5000373,LaSalle 1,LaSalle 1,6026,1,,,,\n", "not an NRC power reactor docket"),
            ("05000373,A,A,1,1,,,,\n05000373,B,B,2,1,,,,\n", "docket 05000373 appears twice"),
            ("05000373,A,A,1,1,,,,\n05000374,B,A,2,1,,,,\n", "NRC name \"A\": appears twice"),
            ("05000373,A,A,1,1,,,,\n05000374,B,B,1,1,,,,\n", "EIA generator"),
            ("05000373,A,A; B,1,1,,,,\n", "blank or padded"),
            ("05000373,A,A,1,1,,2020-01-01,,\n", "has a date but no source"),
            ("05000373,A,A,1,1,2020-01-01,,,\n", "has a date but no source"),
            ("05000373,A,A,1,1,2021-01-01,2020-01-01,,x\n", "shuts down before commercial operation"),
            ("05000373,A,A,1,1,,2020-01-01,2019-01-01,x\n", "restarts without an earlier shutdown"),
            ("05000373,A,A,1,1,,,2019-01-01,x\n", "restarts without an earlier shutdown"),
            ("05000373,A,A,plant,1,,,,\n", "line 2"),
        ];
        for (rows, expected) in cases {
            let error = error(rows);
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }
}
