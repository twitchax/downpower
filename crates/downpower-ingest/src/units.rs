//! The unit spine (DESIGN.md Decision 16): every reactor unit on the status pages since 1999, keyed by
//! its NRC docket, from the committed `catalog/units.csv`. A name the catalog doesn't know fails the
//! backfill, so a new spelling is added on purpose.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

use anyhow::{Context as _, bail, ensure};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::Res;

/// The catalog, embedded so the binary carries it.
pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/units.csv"));

/// An NRC docket number: the license's part in three digits, then five, e.g. `05000275` (50-275,
/// Diablo Canyon 1). Reactors are licensed under Part 50, or 52 (Vogtle 3 and 4, `05200025`); the
/// facilities Event Notifications name add Part 30 (`03000001`), 40 (`04003392`), 70 (`07001113`)
/// and 72 (`07200001`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Docket(String);

/// The parts a docket can name: reactors first.
const PARTS: [&str; 6] = ["050", "052", "030", "040", "070", "072"];

impl Docket {
    /// The eight digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the docket is a reactor's, under Part 50 or 52.
    #[must_use]
    pub fn is_reactor(&self) -> bool {
        PARTS[..2].iter().any(|part| self.0.starts_with(part))
    }
}

impl TryFrom<String> for Docket {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Res<Self> {
        let digits = value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit());
        ensure!(digits && PARTS.iter().any(|part| value.starts_with(part)), "not an NRC docket: {value:?}");
        Ok(Self(value))
    }
}

impl From<Docket> for String {
    fn from(docket: Docket) -> Self {
        docket.0
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
    /// The state the unit is in, as its Event Notifications give it (PRD-0005 T-005).
    pub state: String,
    /// Every spelling the status pages have used for the unit.
    pub nrc_names: Vec<String>,
    /// Every spelling Event Notifications have used for the unit's site, matched ignoring case, and
    /// the unit's number there (PRD-0003 T-004).
    pub en_site: Vec<String>,
    pub en_unit: u8,
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

/// A row of `catalog/units.csv`. `nrc_names` and `en_site` hold every spelling, joined by `;`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvRow {
    docket: String,
    name: String,
    state: String,
    nrc_names: String,
    en_site: String,
    en_unit: u8,
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
    /// When a row doesn't parse, a state isn't two capital letters, a docket is malformed, a docket, NRC name or EIA generator appears
    /// twice, a shutdown has no source, or a restart doesn't follow its shutdown.
    pub fn parse(csv: &str) -> Res<Self> {
        let mut units = Vec::new();
        let mut dockets = HashMap::new();
        let mut by_nrc_name = HashMap::new();
        let mut by_eia = HashMap::new();
        for (index, row) in csv::Reader::from_reader(csv.as_bytes()).deserialize::<CsvRow>().enumerate() {
            let line = index + 2;
            let row = row.with_context(|| format!("line {line}"))?;
            let docket = Docket::try_from(row.docket).with_context(|| format!("line {line}"))?;
            ensure!(docket.is_reactor(), "line {line}: {docket} is not a reactor docket");
            let unit = Unit {
                docket,
                nrc_names: row.nrc_names.split(';').map(str::to_owned).collect(),
                en_site: row.en_site.split(';').map(str::to_owned).collect(),
                en_unit: row.en_unit,
                name: row.name,
                state: row.state,
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
                unit.state.len() == 2 && unit.state.bytes().all(|byte| byte.is_ascii_uppercase()),
                "line {line}: {} has state {:?}",
                unit.name,
                unit.state
            );
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
            for name in &unit.en_site {
                ensure!(!name.is_empty() && name.trim() == name, "line {line}: EN site {name:?} is blank or padded");
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

    const HEADER: &str = "docket,name,nrc_names,en_site,en_unit,eia_plant,eia_generator,commercial,shutdown,restart,source,state\n";

    fn error(rows: &str) -> String {
        format!("{:#}", Units::parse(&format!("{HEADER}{rows}")).unwrap_err())
    }

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn every_key_finds_its_unit() {
        let units = Units::parse(&format!("{HEADER}05000373,LaSalle 1,LaSalle 1;La Salle 1,LaSalle;La Salle,1,6026,1,,,,,IL\n")).unwrap();
        let unit = &units.all()[0];

        assert_eq!(unit.docket.as_str(), "05000373");
        assert_eq!(units.by_nrc_name("La Salle 1"), Some(unit));
        assert_eq!(units.by_nrc_name("LaSalle 1"), Some(unit));
        assert_eq!(units.by_eia(&EiaGenerator { plant: 6026, generator: "1".to_owned() }), Some(unit));
        assert_eq!(units.by_nrc_name("Zion 1"), None);
        assert_eq!(unit.en_site, ["LaSalle", "La Salle"]);
        assert_eq!(unit.en_unit, 1);
        assert_eq!(unit.state, "IL");
    }

    #[test]
    fn dockets_cover_every_part_an_event_names() {
        for (docket, reactor) in [
            ("05000275", true),
            ("05200025", true),
            ("03000001", false),
            ("04003392", false),
            ("07001113", false),
            ("07200001", false),
        ] {
            let docket = Docket::try_from(docket.to_owned()).unwrap();
            assert_eq!(docket.is_reactor(), reactor, "{docket}");
        }
        for docket in ["0500123", "05000O73", "06000001", "070011130"] {
            assert!(Docket::try_from(docket.to_owned()).is_err(), "{docket}");
        }
    }

    #[test]
    fn service_ends_after_the_shutdown_day_and_resumes_at_restart() {
        let units = Units::parse(&format!(
            "{HEADER}05000255,Palisades,Palisades,Palisades,1,1715,1,,2022-05-20,2025-10-01,test,IL\n05000271,Vermont Yankee,Vermont Yankee,Vermont Yankee,1,3751,1,,2014-12-29,,test,IL\n05200025,Vogtle 3,Vogtle 3,Vogtle,3,649,3,2023-07-31,,,test,IL\n"
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
            ("5000373,LaSalle 1,LaSalle 1,LaSalle,1,6026,1,,,,,IL\n", "not an NRC docket"),
            ("07001113,LaSalle 1,LaSalle 1,LaSalle,1,6026,1,,,,,IL\n", "not a reactor docket"),
            ("05000373,A,A,A,1,1,1,,,,,IL\n05000373,B,B,B,1,2,1,,,,,IL\n", "docket 05000373 appears twice"),
            ("05000373,A,A,A,1,1,1,,,,,IL\n05000374,B,A,B,1,2,1,,,,,IL\n", "NRC name \"A\": appears twice"),
            ("05000373,A,A,A,1,1,1,,,,,IL\n05000374,B,B,B,1,1,1,,,,,IL\n", "EIA generator"),
            ("05000373,A,A; B,A,1,1,1,,,,,IL\n", "blank or padded"),
            ("05000373,A,A,A; B,1,1,1,,,,,IL\n", "EN site \" B\" is blank or padded"),
            ("05000373,A,A,,1,1,1,,,,,IL\n", "EN site \"\" is blank or padded"),
            ("05000373,A,A,A,,1,1,,,,,IL\n", "line 2"),
            ("05000373,A,A,A,1,1,1,,2020-01-01,,,IL\n", "has a date but no source"),
            ("05000373,A,A,A,1,1,1,2020-01-01,,,,IL\n", "has a date but no source"),
            ("05000373,A,A,A,1,1,1,2021-01-01,2020-01-01,,x,IL\n", "shuts down before commercial operation"),
            ("05000373,A,A,A,1,1,1,,2020-01-01,2019-01-01,x,IL\n", "restarts without an earlier shutdown"),
            ("05000373,A,A,A,1,1,1,,,2019-01-01,x,IL\n", "restarts without an earlier shutdown"),
            ("05000373,A,A,A,1,plant,1,,,,,IL\n", "line 2"),
            ("05000373,A,A,A,1,1,1,,,,,Ill\n", "state \"Ill\""),
        ];
        for (rows, expected) in cases {
            let error = error(rows);
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }
}
