//! Facility identity (DESIGN.md Decision 31): which reactor units, facility or licensee an Event
//! Notification is about.
//!
//! - **Power reactor events** name a site and its unit numbers. Both map to dockets through the
//!   EN names in `catalog/units.csv` and, for reactors outside the ledger (shut down before 1999,
//!   or never finished), `catalog/facilities.csv`.
//! - **Fuel cycle facilities** match on (state, name), since a name can follow an owner from one
//!   facility to another, and **non-power reactors** on their license number. Both are keyed by
//!   docket in `catalog/facilities.csv`.
//! - **Every other licensee** groups by (state, license number), under the group's newest name.
//!   Part 21 vendors and licensees with no license number stay text.
//!
//! Names match ignoring case and runs of spaces, since pages flip to title case in 2020. A name the
//! catalogs don't know fails the backfill and is quarantined by the server (Decision 21).

use std::collections::hash_map::Entry as Slot;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use anyhow::{Context as _, bail, ensure};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::Res;
use crate::events::{Category, Event, Scram, Site};
use crate::ledger::Unknowns;
use crate::units::{Docket, Units};

/// The catalog, embedded so the binary carries it.
pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/facilities.csv"));

/// What a `catalog/facilities.csv` row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A power reactor unit outside the ledger, matched by its EN site name and unit number.
    PowerReactor,
    /// A fuel cycle facility (spent fuel storage included), matched by state and EN name.
    FuelCycle,
    /// A research or test reactor, matched by license number. The NRC filed these as Research
    /// Reactor events until 2004 and as Non-Power Reactor events since.
    NonPowerReactor,
}

/// One `catalog/facilities.csv` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub docket: Docket,
    pub kind: Kind,
    /// The name it is shown under: the current one, for a facility that changed hands.
    pub name: String,
    pub state: String,
    /// Every spelling Event Notifications have used: a reactor's site, or a fuel cycle facility
    /// under each of its owners.
    pub en_names: Vec<String>,
    /// A power reactor's unit number on the event pages.
    pub unit: Option<u8>,
    /// A non-power reactor's license, e.g. `R-103`.
    pub license: Option<String>,
    /// Where the docket comes from.
    pub source: String,
}

/// A row of `catalog/facilities.csv`. `en_names` holds every spelling, joined by `;`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvRow {
    docket: String,
    kind: Kind,
    name: String,
    state: String,
    en_names: Option<String>,
    unit: Option<u8>,
    license: Option<String>,
    source: Option<String>,
}

/// Who an event is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Identity {
    /// A power reactor event: the docket of each unit in its unit table, in the table's order. An
    /// event about a whole site lists no units.
    Reactor { dockets: Vec<Docket> },
    /// A fuel cycle facility or non-power reactor, under its catalog name.
    Facility { docket: Docket, name: String },
    /// A licensee, grouped by state and license number (its letters and digits, upper case), and
    /// shown under the name on the group's newest event.
    Licensee { state: String, license: String, name: Option<String> },
}

/// An event whose facility the catalogs don't know, described for the error or the quarantine
/// count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown(pub String);

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A reactor event's row for one unit, which the barcode's markers read (PRD-0004).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventUnit {
    pub event: u32,
    pub unit: u8,
    pub docket: Docket,
    /// The day the event happened.
    pub occurred: NaiveDate,
    pub scram: Option<Scram>,
    pub retracted: bool,
}

/// The identity catalogs, indexed by every key an event page uses.
#[derive(Debug, Clone)]
pub struct Facilities {
    all: Vec<Entry>,
    /// (site, unit) to docket, over both catalogs.
    reactors: HashMap<(String, u8), Docket>,
    sites: HashSet<String>,
    /// (state, name) to the entry.
    fuel_cycle: HashMap<(String, String), usize>,
    /// License number to the entry.
    licenses: HashMap<String, usize>,
}

impl Facilities {
    /// The committed catalog, beside the units' EN names.
    ///
    /// # Errors
    ///
    /// When `catalog/facilities.csv` breaks a rule [`Facilities::parse`] checks.
    pub fn catalog(units: &Units) -> Res<Self> {
        Self::parse(CATALOG, units).context("catalog/facilities.csv")
    }

    /// Parses a facilities table and indexes it with `units`' EN names.
    ///
    /// # Errors
    ///
    /// When a row doesn't parse or doesn't carry what its kind needs, or a docket, a reactor's site
    /// and unit, a fuel cycle facility's state and name, or a license appears twice.
    pub fn parse(csv: &str, units: &Units) -> Res<Self> {
        let mut facilities = Self {
            all: Vec::new(),
            reactors: HashMap::new(),
            sites: HashSet::new(),
            fuel_cycle: HashMap::new(),
            licenses: HashMap::new(),
        };
        for unit in units.all() {
            for site in &unit.en_site {
                facilities.add_reactor(site, unit.en_unit, &unit.docket).with_context(|| format!("catalog/units.csv, {}", unit.name))?;
            }
        }
        let mut dockets = HashSet::new();
        for (index, row) in csv::Reader::from_reader(csv.as_bytes()).deserialize::<CsvRow>().enumerate() {
            let line = index + 2;
            let entry = Entry::try_from(row.with_context(|| format!("line {line}"))?).with_context(|| format!("line {line}"))?;
            ensure!(dockets.insert(entry.docket.clone()), "line {line}: docket {} appears twice", entry.docket);
            facilities.add(entry).with_context(|| format!("line {line}"))?;
        }
        Ok(facilities)
    }

    fn add_reactor(&mut self, site: &str, unit: u8, docket: &Docket) -> Res<()> {
        self.sites.insert(key(site));
        insert_unique(&mut self.reactors, (key(site), unit), docket.clone()).with_context(|| format!("Reactor {site:?} unit {unit}"))
    }

    fn add(&mut self, entry: Entry) -> Res<()> {
        let slot = self.all.len();
        match entry.kind {
            Kind::PowerReactor => {
                let unit = entry.unit.context("a power_reactor row needs a unit")?;
                for site in &entry.en_names {
                    self.add_reactor(site, unit, &entry.docket)?;
                }
            }
            Kind::FuelCycle => {
                for name in &entry.en_names {
                    insert_unique(&mut self.fuel_cycle, (entry.state.clone(), key(name)), slot).with_context(|| format!("Fuel cycle facility {name:?} in {}", entry.state))?;
                }
            }
            Kind::NonPowerReactor => {
                let license = entry.license.as_deref().context("a non_power_reactor row needs a license")?;
                let normal = license_key(license).with_context(|| format!("license {license:?} has no digit"))?;
                insert_unique(&mut self.licenses, normal, slot).with_context(|| format!("License {license:?}"))?;
            }
        }
        self.all.push(entry);
        Ok(())
    }

    /// Every entry, in catalog order.
    #[must_use]
    pub fn all(&self) -> &[Entry] {
        &self.all
    }

    /// Who `event` is about: `None` for a Part 21 vendor, or a licensee with no state or no license
    /// number.
    ///
    /// # Errors
    ///
    /// When the catalogs don't know the reactor site or unit, the fuel cycle facility, or the
    /// non-power reactor's license.
    pub fn identify(&self, event: &Event) -> Result<Option<Identity>, Unknown> {
        let category = event.category.label();
        match (event.category, &event.site) {
            (Category::PowerReactor, Site::Facility(facility)) => {
                let site = key(&facility.name);
                if event.units.is_empty() && !self.sites.contains(&site) {
                    return Err(Unknown(format!("{category} {:?}", facility.name)));
                }
                let dockets = event
                    .units
                    .iter()
                    .map(|info| {
                        self.reactors
                            .get(&(site.clone(), info.unit))
                            .cloned()
                            .ok_or_else(|| Unknown(format!("{category} {:?} unit {}", facility.name, info.unit)))
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Some(Identity::Reactor { dockets }))
            }
            (Category::FuelCycleFacility, Site::Facility(facility)) => {
                let state = event.state.clone().unwrap_or_default();
                let found = self.fuel_cycle.get(&(state, key(&facility.name)));
                let entry = found
                    .map(|&slot| &self.all[slot])
                    .ok_or_else(|| Unknown(format!("{category} {:?} in {}", facility.name, event.state.as_deref().unwrap_or("no state"))))?;
                Ok(Some(entry.identity()))
            }
            (Category::NonPowerReactor | Category::ResearchReactor, Site::Licensee(licensee)) => {
                let Some(license) = &licensee.license else {
                    return Err(Unknown(format!("{category} {:?} with no license", licensee.name.as_deref().unwrap_or_default())));
                };
                let found = license_key(license).and_then(|normal| self.licenses.get(&normal));
                let entry = found.map(|&slot| &self.all[slot]).ok_or_else(|| Unknown(format!("{category} license {license:?}")))?;
                Ok(Some(entry.identity()))
            }
            (Category::Part21, Site::Licensee(_)) => Ok(None),
            (Category::PowerReactor | Category::FuelCycleFacility, Site::Licensee(_)) | (_, Site::Facility(_)) => Err(Unknown(format!("{category} event under the other header"))),
            (_, Site::Licensee(licensee)) => Ok(event.state.as_ref().zip(licensee.license.as_deref().and_then(license_key)).map(|(state, license)| Identity::Licensee {
                state: state.clone(),
                license,
                name: licensee.name.clone(),
            })),
        }
    }

    /// Identifies every event, and names each licensee group after its newest event that has a
    /// name. An unknown facility fails under [`Unknowns::Fail`]; under [`Unknowns::Quarantine`] it
    /// is counted in `unknown` and its event stays unidentified.
    ///
    /// # Errors
    ///
    /// On an unknown facility under [`Unknowns::Fail`].
    pub fn identify_all(&self, events: &BTreeMap<u32, Event>, unknowns: Unknowns, unknown: &mut BTreeMap<String, u64>) -> Res<BTreeMap<u32, Identity>> {
        let mut identities = BTreeMap::new();
        for (number, event) in events {
            match self.identify(event) {
                Ok(Some(identity)) => {
                    identities.insert(*number, identity);
                }
                Ok(None) => {}
                Err(found) if unknowns == Unknowns::Quarantine => *unknown.entry(found.0).or_default() += 1,
                Err(found) => bail!("event {number}: unknown {found}; add it to catalog/units.csv or catalog/facilities.csv"),
            }
        }
        // Numbers rise with time, so the last name a group meets in number order is its newest.
        let mut newest: HashMap<(String, String), String> = HashMap::new();
        for identity in identities.values() {
            if let Identity::Licensee { state, license, name: Some(name) } = identity {
                newest.insert((state.clone(), license.clone()), name.clone());
            }
        }
        for identity in identities.values_mut() {
            if let Identity::Licensee { state, license, name } = identity
                && let Some(newest) = newest.get(&(state.clone(), license.clone()))
            {
                *name = Some(newest.clone());
            }
        }
        Ok(identities)
    }
}

/// A reactor event's rows, one per unit in its table, each with that unit's docket.
#[must_use]
pub fn event_units(event: &Event, identity: Option<&Identity>) -> Vec<EventUnit> {
    let Some(Identity::Reactor { dockets }) = identity else {
        return Vec::new();
    };
    event
        .units
        .iter()
        .zip(dockets)
        .map(|(info, docket)| EventUnit {
            event: event.number,
            unit: info.unit,
            docket: docket.clone(),
            occurred: event.occurred.date,
            scram: info.scram,
            retracted: event.retracted,
        })
        .collect()
}

impl Entry {
    fn identity(&self) -> Identity {
        Identity::Facility {
            docket: self.docket.clone(),
            name: self.name.clone(),
        }
    }
}

impl TryFrom<CsvRow> for Entry {
    type Error = anyhow::Error;

    fn try_from(row: CsvRow) -> Res<Self> {
        let docket = Docket::try_from(row.docket)?;
        let kind = row.kind;
        let slug = match kind {
            Kind::PowerReactor => "power_reactor",
            Kind::FuelCycle => "fuel_cycle",
            Kind::NonPowerReactor => "non_power_reactor",
        };
        let source = row.source.with_context(|| format!("{docket} has no source"))?;
        ensure!(!row.name.is_empty() && row.name.trim() == row.name, "name {:?} is blank or padded", row.name);
        ensure!(
            row.state.len() == 2 && row.state.bytes().all(|byte| byte.is_ascii_uppercase()),
            "state {:?} is not a two-letter code",
            row.state
        );
        let en_names: Vec<String> = row.en_names.iter().flat_map(|names| names.split(';')).map(str::to_owned).collect();
        for name in &en_names {
            ensure!(!name.is_empty() && name.trim() == name, "EN name {name:?} is blank or padded");
        }
        match kind {
            Kind::PowerReactor | Kind::NonPowerReactor => ensure!(docket.is_reactor(), "{docket} is not a reactor docket"),
            Kind::FuelCycle => {}
        }
        match kind {
            Kind::PowerReactor | Kind::FuelCycle => {
                ensure!(!en_names.is_empty(), "a {slug} row needs EN names");
                ensure!(row.license.is_none(), "a {slug} row takes no license");
            }
            Kind::NonPowerReactor => {
                ensure!(en_names.is_empty(), "a {slug} row takes no EN names");
                ensure!(row.unit.is_none(), "a {slug} row takes no unit");
            }
        }
        if kind == Kind::FuelCycle {
            ensure!(row.unit.is_none(), "a {slug} row takes no unit");
        }
        Ok(Self {
            docket,
            kind,
            name: row.name,
            state: row.state,
            en_names,
            unit: row.unit,
            license: row.license,
            source,
        })
    }
}

/// A name as matched: upper case, with runs of spaces collapsed.
fn key(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

/// A license number as grouped: its letters and digits, upper case. `None` for a placeholder with no
/// digit in it (`GL`, `N/A`, `General License`).
fn license_key(license: &str) -> Option<String> {
    let normal: String = license.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    normal.bytes().any(|byte| byte.is_ascii_digit()).then_some(normal)
}

fn insert_unique<K: std::hash::Hash + Eq, V>(map: &mut HashMap<K, V>, key: K, value: V) -> Res<()> {
    match map.entry(key) {
        Slot::Occupied(_) => bail!("appears twice"),
        Slot::Vacant(slot) => {
            slot.insert(value);
            Ok(())
        }
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use chrono::NaiveTime;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::events::{Facility, Licensee, Stamp, UnitInfo};

    const UNITS: &str = "docket,name,nrc_names,en_site,en_unit,eia_plant,eia_generator,commercial,shutdown,restart,source
05000528,Palo Verde 1,Palo Verde 1,Palo Verde,1,6008,1,,,,
05000529,Palo Verde 2,Palo Verde 2,Palo Verde,2,6008,2,,,,
05000237,Dresden 2,Dresden 2,Dresden,2,869,2,,,,
";
    const HEADER: &str = "docket,kind,name,state,en_names,unit,license,source\n";
    const FACILITIES: &str = "05000010,power_reactor,Dresden 1,IL,Dresden,1,,test
05000295,power_reactor,Zion 1,IL,Zion,1,,test
07200001,fuel_cycle,GE Morris Operation,IL,General Electric Co.,,,test
07000754,fuel_cycle,GE Vallecitos Nuclear Center,CA,General Electric Co.,,,test
07001257,fuel_cycle,Framatome Richland,WA,Siemens Power Corporation;Framatome ANP Richland,,,test
05000123,non_power_reactor,Missouri S&T,MO,,,R-79,test
";

    fn facilities() -> Facilities {
        Facilities::parse(&format!("{HEADER}{FACILITIES}"), &Units::parse(UNITS).unwrap()).unwrap()
    }

    fn docket(text: &str) -> Docket {
        Docket::try_from(text.to_owned()).unwrap()
    }

    fn stamp(date: &str) -> Stamp {
        Stamp {
            date: date.parse().unwrap(),
            time: NaiveTime::MIN,
            zone: None,
        }
    }

    fn event(number: u32, category: Category, state: Option<&str>, site: Site) -> Event {
        Event {
            number,
            category,
            retracted: false,
            site,
            region: 1,
            state: state.map(str::to_owned),
            notified_by: None,
            ops_officer: None,
            notified: stamp("2020-01-02"),
            occurred: stamp("2020-01-01"),
            updated: "2020-01-02".parse().unwrap(),
            class: None,
            sections: Vec::new(),
            persons: Vec::new(),
            units: Vec::new(),
            title: None,
            text: String::new(),
            updates: Vec::new(),
            revised: None,
        }
    }

    fn unit_info(unit: u8, scram: Option<Scram>) -> UnitInfo {
        UnitInfo {
            unit,
            scram,
            critical: true,
            initial_power: 100,
            initial_mode: None,
            current_power: 0,
            current_mode: None,
        }
    }

    fn reactor(name: &str, units: &[u8]) -> Event {
        let mut event = event(
            1,
            Category::PowerReactor,
            Some("AZ"),
            Site::Facility(Facility {
                name: name.to_owned(),
                units: units.to_vec(),
                rx_type: None,
                comments: None,
            }),
        );
        event.units = units.iter().map(|&unit| unit_info(unit, Some(Scram::No))).collect();
        event
    }

    fn fuel_cycle(name: &str, state: &str) -> Event {
        event(
            1,
            Category::FuelCycleFacility,
            Some(state),
            Site::Facility(Facility {
                name: name.to_owned(),
                units: Vec::new(),
                rx_type: None,
                comments: None,
            }),
        )
    }

    fn licensee(number: u32, category: Category, state: Option<&str>, name: Option<&str>, license: Option<&str>) -> Event {
        event(
            number,
            category,
            state,
            Site::Licensee(Licensee {
                rep_org: None,
                name: name.map(str::to_owned),
                city: None,
                county: None,
                license: license.map(str::to_owned),
                agreement: true,
                docket: None,
            }),
        )
    }

    fn reactor_dockets(dockets: &[&str]) -> Identity {
        Identity::Reactor {
            dockets: dockets.iter().map(|text| docket(text)).collect(),
        }
    }

    #[test]
    fn reactor_units_map_to_their_dockets_in_either_catalog_and_either_case() {
        let facilities = facilities();

        assert_eq!(facilities.identify(&reactor("PALO VERDE", &[1, 2])), Ok(Some(reactor_dockets(&["05000528", "05000529"]))));
        assert_eq!(facilities.identify(&reactor("Palo  verde", &[2])), Ok(Some(reactor_dockets(&["05000529"]))));
        assert_eq!(facilities.identify(&reactor("DRESDEN", &[1, 2])), Ok(Some(reactor_dockets(&["05000010", "05000237"]))));
        assert_eq!(facilities.identify(&reactor("Zion", &[1])), Ok(Some(reactor_dockets(&["05000295"]))));
    }

    #[test]
    fn a_site_event_with_no_units_maps_to_none_of_them() {
        assert_eq!(facilities().identify(&reactor("Palo Verde", &[])), Ok(Some(reactor_dockets(&[]))));
    }

    #[test]
    fn an_unknown_site_or_unit_is_unknown() {
        let facilities = facilities();
        let cases = [
            (reactor("PALO VERDE", &[1, 3]), "Power Reactor \"PALO VERDE\" unit 3"),
            (reactor("TROJAN", &[1]), "Power Reactor \"TROJAN\" unit 1"),
            (reactor("TROJAN", &[]), "Power Reactor \"TROJAN\""),
        ];
        for (event, expected) in cases {
            assert_eq!(facilities.identify(&event), Err(Unknown(expected.to_owned())));
        }
    }

    #[test]
    fn fuel_cycle_facilities_match_on_state_and_name() {
        let facilities = facilities();
        let facility = |text: &str, name: &str| {
            Ok(Some(Identity::Facility {
                docket: docket(text),
                name: name.to_owned(),
            }))
        };

        assert_eq!(facilities.identify(&fuel_cycle("GENERAL ELECTRIC CO.", "IL")), facility("07200001", "GE Morris Operation"));
        assert_eq!(facilities.identify(&fuel_cycle("GENERAL ELECTRIC CO.", "CA")), facility("07000754", "GE Vallecitos Nuclear Center"));
        assert_eq!(facilities.identify(&fuel_cycle("SIEMENS POWER CORPORATION", "WA")), facility("07001257", "Framatome Richland"));
        assert_eq!(facilities.identify(&fuel_cycle("Framatome Anp Richland", "WA")), facility("07001257", "Framatome Richland"));
        assert_eq!(
            facilities.identify(&fuel_cycle("GENERAL ELECTRIC CO.", "NC")),
            Err(Unknown("Fuel Cycle Facility \"GENERAL ELECTRIC CO.\" in NC".to_owned()))
        );
    }

    #[test]
    fn non_power_reactors_match_on_their_license_whatever_the_docket_field_says() {
        let facilities = facilities();
        let mut rolla = licensee(1, Category::NonPowerReactor, Some("MO"), Some("Missouri University Of Science And Technology"), Some("R-79"));
        if let Site::Licensee(licensee) = &mut rolla.site {
            licensee.docket = Some("0500123".to_owned());
        }

        assert_eq!(
            facilities.identify(&rolla),
            Ok(Some(Identity::Facility {
                docket: docket("05000123"),
                name: "Missouri S&T".to_owned()
            }))
        );
        assert_eq!(
            facilities.identify(&licensee(1, Category::ResearchReactor, Some("MO"), None, Some("r 79"))),
            facilities.identify(&rolla)
        );
        assert_eq!(
            facilities.identify(&licensee(1, Category::NonPowerReactor, Some("MO"), Some("X"), Some("R-80"))),
            Err(Unknown("Non-Power Reactor license \"R-80\"".to_owned()))
        );
        assert_eq!(
            facilities.identify(&licensee(1, Category::NonPowerReactor, Some("MO"), Some("X"), None)),
            Err(Unknown("Non-Power Reactor \"X\" with no license".to_owned()))
        );
    }

    #[test]
    fn licensees_group_by_state_and_the_letters_and_digits_of_their_license() {
        let facilities = facilities();
        let group = |state: &str, license: &str, name: &str| {
            Ok(Some(Identity::Licensee {
                state: state.to_owned(),
                license: license.to_owned(),
                name: Some(name.to_owned()),
            }))
        };

        assert_eq!(
            facilities.identify(&licensee(1, Category::AgreementState, Some("MI"), Some("A"), Some("21-01190.05"))),
            group("MI", "210119005", "A")
        );
        assert_eq!(
            facilities.identify(&licensee(1, Category::Hospital, Some("TX"), Some("B"), Some("l-02187 "))),
            group("TX", "L02187", "B")
        );
        for category in [
            Category::NonAgreementState,
            Category::OtherNuclearMaterial,
            Category::GeneralInformationOrOther,
            Category::GeneralInformation,
            Category::SpentFuelStorage,
            Category::Transportation,
        ] {
            assert_eq!(facilities.identify(&licensee(1, category, Some("MI"), Some("C"), Some("21-1"))), group("MI", "211", "C"));
        }
    }

    #[test]
    fn part_21_vendors_and_licensees_without_a_license_number_or_state_stay_text() {
        let facilities = facilities();
        let cases = [
            licensee(1, Category::Part21, Some("PA"), Some("Westinghouse"), Some("21-0001")),
            licensee(1, Category::AgreementState, Some("CO"), Some("A"), Some("General License")),
            licensee(1, Category::AgreementState, Some("CO"), Some("A"), Some("N/A")),
            licensee(1, Category::AgreementState, Some("CO"), Some("A"), None),
            licensee(1, Category::AgreementState, None, Some("A"), Some("21-0001")),
        ];
        for event in cases {
            assert_eq!(facilities.identify(&event), Ok(None), "{:?}", event.site);
        }
    }

    #[test]
    fn a_group_takes_the_name_on_its_newest_event_with_one() {
        let facilities = facilities();
        let events: BTreeMap<u32, Event> = [
            licensee(10, Category::AgreementState, Some("IL"), Some("U S ARMY"), Some("12-0072206")),
            licensee(30, Category::AgreementState, Some("IL"), Some("DEPARTMENT OF ARMY"), Some("120072206")),
            licensee(40, Category::AgreementState, Some("IL"), None, Some("12-00722-06")),
            licensee(20, Category::AgreementState, Some("IN"), Some("US ARMY TACOM"), Some("12-0072206")),
        ]
        .into_iter()
        .map(|event| (event.number, event))
        .collect();

        let identities = facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap();

        let names: Vec<_> = identities
            .iter()
            .map(|(number, identity)| match identity {
                Identity::Licensee { state, name, .. } => (*number, state.as_str(), name.as_deref()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            names,
            [
                (10, "IL", Some("DEPARTMENT OF ARMY")),
                (20, "IN", Some("US ARMY TACOM")),
                (30, "IL", Some("DEPARTMENT OF ARMY")),
                (40, "IL", Some("DEPARTMENT OF ARMY"))
            ]
        );
    }

    #[test]
    fn an_unknown_facility_fails_the_backfill_and_is_counted_by_the_server() {
        let facilities = facilities();
        let mut trojan = reactor("TROJAN", &[1]);
        trojan.number = 7;
        let events: BTreeMap<u32, Event> = [(1, reactor("PALO VERDE", &[1])), (7, trojan)].into_iter().collect();

        let error = facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap_err();
        assert!(
            format!("{error:#}").contains("event 7: unknown Power Reactor \"TROJAN\" unit 1; add it to catalog/units.csv or catalog/facilities.csv"),
            "{error:#}"
        );

        let mut unknown = BTreeMap::new();
        let identities = facilities.identify_all(&events, Unknowns::Quarantine, &mut unknown).unwrap();
        assert_eq!(identities.keys().collect::<Vec<_>>(), [&1]);
        assert_eq!(unknown, BTreeMap::from([("Power Reactor \"TROJAN\" unit 1".to_owned(), 1)]));
    }

    #[test]
    fn a_reactor_event_has_a_row_per_unit_with_its_docket() {
        let mut event = reactor("PALO VERDE", &[1, 2]);
        event.number = 35211;
        event.retracted = true;
        event.units[1].scram = Some(Scram::AutoReactor);
        let identity = facilities().identify(&event).unwrap();

        let rows = event_units(&event, identity.as_ref());

        let row = |unit: u8, text: &str, scram: Scram| EventUnit {
            event: 35211,
            unit,
            docket: docket(text),
            occurred: "2020-01-01".parse().unwrap(),
            scram: Some(scram),
            retracted: true,
        };
        assert_eq!(rows, [row(1, "05000528", Scram::No), row(2, "05000529", Scram::AutoReactor)]);
        assert_eq!(event_units(&event, None), []);
        assert_eq!(
            event_units(
                &fuel_cycle("GENERAL ELECTRIC CO.", "IL"),
                Some(&Identity::Facility {
                    docket: docket("07200001"),
                    name: String::new()
                })
            ),
            []
        );
    }

    #[test]
    fn the_table_is_checked() {
        let units = Units::parse(UNITS).unwrap();
        let cases = [
            ("0700754,fuel_cycle,A,CA,A,,,test\n", "not an NRC docket"),
            ("07000754,fuel_cycle,A,CA,A,,,test\n07000754,fuel_cycle,B,CA,B,,,test\n", "docket 07000754 appears twice"),
            ("07000754,fuel_cycle,A,CA,A,,,\n", "has no source"),
            ("07000754,fuel_cycle, A,CA,A,,,test\n", "name \" A\" is blank or padded"),
            ("07000754,fuel_cycle,A,California,A,,,test\n", "state \"California\""),
            ("07000754,fuel_cycle,A,CA,A;,,,test\n", "EN name \"\" is blank or padded"),
            ("07000754,fuel_cycle,A,CA,,,,test\n", "a fuel_cycle row needs EN names"),
            ("07000754,fuel_cycle,A,CA,A,1,,test\n", "a fuel_cycle row takes no unit"),
            ("07000754,fuel_cycle,A,CA,A,,R-1,test\n", "a fuel_cycle row takes no license"),
            (
                "07000754,fuel_cycle,A,CA,A,,,test\n07000755,fuel_cycle,B,CA,a,,,test\n",
                "Fuel cycle facility \"a\" in CA: appears twice",
            ),
            ("07000754,power_reactor,A,CA,A,1,,test\n", "not a reactor docket"),
            ("05000010,power_reactor,A,CA,A,,,test\n", "a power_reactor row needs a unit"),
            ("05000010,power_reactor,A,CA,,1,,test\n", "a power_reactor row needs EN names"),
            ("05000010,power_reactor,A,CA,A,1,R-1,test\n", "a power_reactor row takes no license"),
            ("05000010,power_reactor,A,AZ,PALO VERDE,1,,test\n", "Reactor \"PALO VERDE\" unit 1: appears twice"),
            ("05000010,non_power_reactor,A,CA,,,,test\n", "a non_power_reactor row needs a license"),
            ("05000010,non_power_reactor,A,CA,A,,R-1,test\n", "a non_power_reactor row takes no EN names"),
            ("05000010,non_power_reactor,A,CA,,1,R-1,test\n", "a non_power_reactor row takes no unit"),
            ("05000010,non_power_reactor,A,CA,,,R-,test\n", "license \"R-\" has no digit"),
            ("07000754,non_power_reactor,A,CA,,,R-1,test\n", "not a reactor docket"),
            (
                "05000010,non_power_reactor,A,CA,,,R-1,test\n05000011,non_power_reactor,B,CA,,,r 1,test\n",
                "License \"r 1\": appears twice",
            ),
            ("05000010,breeder,A,CA,,,R-1,test\n", "line 2"),
        ];
        for (rows, expected) in cases {
            let error = format!("{:#}", Facilities::parse(&format!("{HEADER}{rows}"), &units).unwrap_err());
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }
}
