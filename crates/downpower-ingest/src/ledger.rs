//! The ledger: one row per in-service unit-day, built from the raw store by the one pipeline the
//! backfill, the rebuild and the daily update share (DESIGN.md Decisions 7, 15 and 20). It streams
//! day by day, so memory holds one page, the previous page's rows and each unit's attribution
//! state, never the archive.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context as _, bail, ensure};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::attribution::{self, Attributed, Attributor, Evidence, Method};
use crate::capacity::{self, Annual};
use crate::eia::NucStatus;
use crate::raw::{RawStore, Series};
use crate::status::{self, Row, StatusPage};
use crate::tags::{Family, Tags};
use crate::units::{Unit, Units};
use crate::{Res, Void};

/// Bumped whenever parsing, observing, attributing or the row shape changes, so the deployed
/// server rebuilds (Decision 15). The catalog files are hashed in beside it.
pub const LEDGER_VERSION: u32 = 2;

/// The committed tables a build reads, embedded in the binary.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub tags: Tags,
    pub units: Units,
    pub capacity: Annual,
}

impl Catalog {
    /// The tables in `catalog/`.
    ///
    /// # Errors
    ///
    /// When a table breaks a rule its parser checks.
    pub fn embedded() -> Res<Self> {
        Ok(Self {
            tags: Tags::catalog()?,
            units: Units::catalog()?,
            capacity: Annual::catalog()?,
        })
    }
}

/// The stamp a build carries: 16 hex digits over [`LEDGER_VERSION`] and the catalog files the
/// binary embeds.
#[must_use]
pub fn derived_version() -> String {
    let mut hasher = Sha256::new();
    hasher.update(LEDGER_VERSION.to_le_bytes());
    for catalog in [crate::tags::TAGS, crate::tags::UNCLASSIFIED, crate::units::CATALOG, capacity::CATALOG] {
        // Length-prefixed, so moving text from one file to the next changes the stamp.
        hasher.update((catalog.len() as u64).to_le_bytes());
        hasher.update(catalog.as_bytes());
    }
    hex::encode(&hasher.finalize()[..8])
}

/// Where a unit-day's power comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The NRC status page, which wins wherever it exists.
    NrcPage,
    /// EIA's `NUC_STATUS`, on a day the NRC published no report for the unit.
    Eia,
}

/// A cause the observed layer found, with the tag that named it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCause {
    pub cause: String,
    pub tag: String,
}

/// One in-service unit-day: the numbers, the observed layer and the attributed layer (Decision 20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitDay {
    pub docket: String,
    pub date: NaiveDate,
    /// The Power column, or EIA's power on a day only EIA reports.
    pub power: u8,
    pub source: Source,
    /// EIA's power for the day, where it has one, for the disagreement rate.
    pub eia_power: Option<u8>,
    /// The Reason or Comment cell as written. `None` when blank, inside the 28-day hold, and on
    /// EIA days; the method says which.
    pub reason: Option<String>,
    pub down: Option<NaiveDate>,
    pub changed: bool,
    pub scrams: u8,
    /// Observed: each cause a tag names, in reason order.
    pub causes: Vec<ObservedCause>,
    pub flags: Vec<String>,
    pub transition: bool,
    pub xrefs: Vec<Option<u32>>,
    /// Tags accepted on purpose as Unattributed (`catalog/unclassified.tsv`).
    pub unclassified: Vec<String>,
    /// Tags nothing knows, on a quarantined row.
    pub unknown: Vec<String>,
    /// Attributed: the primary cause, `None` when Unattributed.
    pub cause: Option<String>,
    pub family: Family,
    pub method: Method,
    pub evidence: Evidence,
    /// Net capacity that day in kW: EIA-860 before 2007, `NUC_STATUS` from 2007 on (Decision 22).
    pub capacity_kw: Option<u32>,
    /// The reported power reduction as energy, (100 − power)% × capacity × 24 h, rounded down to
    /// the kWh. Zero when the attributed cause books no lost output, `None` without a capacity.
    pub lost_kwh: Option<u64>,
}

impl UnitDay {
    /// A row with numbers and no reason to observe.
    fn numbers(unit: &Unit, date: NaiveDate, power: u8, source: Source, eia_power: Option<u8>, method: Method) -> Self {
        Self {
            docket: unit.docket.as_str().to_owned(),
            date,
            power,
            source,
            eia_power,
            reason: None,
            down: None,
            changed: false,
            scrams: 0,
            causes: Vec::new(),
            flags: Vec::new(),
            transition: false,
            xrefs: Vec::new(),
            unclassified: Vec::new(),
            unknown: Vec::new(),
            cause: None,
            family: Family::Unattributed,
            method,
            evidence: Evidence::None,
            capacity_kw: None,
            lost_kwh: None,
        }
    }
}

/// (100 − `power`)% of `capacity_kw` for 24 hours, in kWh, rounded down.
fn lost_kwh(power: u8, capacity_kw: u32) -> u64 {
    u64::from(100 - power.min(100)) * u64::from(capacity_kw) * 24 / 100
}

/// What to do with a unit name or tag the catalog doesn't know (Decisions 16 and 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unknowns {
    /// Fail the build: the backfill and an operator's rebuild, where someone is there to extend the
    /// catalog.
    Fail,
    /// Quarantine: a row with an unknown tag is attributed `unknown_tag`, and a row naming an
    /// unknown unit is skipped. Both are counted. The server's paths, so one new tag can't freeze
    /// the site.
    Quarantine,
}

/// What a build did.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Rows built, and the first and last day they cover.
    pub days: u64,
    pub first: Option<NaiveDate>,
    pub last: Option<NaiveDate>,
    /// Rows whose power comes from EIA alone.
    pub from_eia: u64,
    /// Rows inside the 28-day hold.
    pub pending: u64,
    /// NRC rows EIA also reports, and how many of those disagree on power.
    pub compared: u64,
    pub disagreed: u64,
    /// Units a page lists more than once.
    pub duplicates: u64,
    /// Quarantined tags and unit names, with their uses.
    pub unknown_tags: BTreeMap<String, u64>,
    pub unknown_units: BTreeMap<String, u64>,
    /// The lost energy across every row, and the rows below 100% with no capacity to weigh them.
    pub lost_kwh: u64,
    pub without_capacity: u64,
}

/// Builds the ledger one day at a time, in date order.
#[derive(Debug)]
pub struct Builder<'a> {
    catalog: &'a Catalog,
    nuc_status: Option<&'a NucStatus>,
    unknowns: Unknowns,
    attributors: HashMap<&'a str, Attributor<'a>>,
    /// Each unit's row on the previous published page, which a stale repeat copies.
    previous: HashMap<&'a str, Row>,
    summary: Summary,
}

impl<'a> Builder<'a> {
    #[must_use]
    pub fn new(catalog: &'a Catalog, nuc_status: Option<&'a NucStatus>, unknowns: Unknowns) -> Self {
        Self {
            catalog,
            nuc_status,
            unknowns,
            attributors: HashMap::new(),
            previous: HashMap::new(),
            summary: Summary::default(),
        }
    }

    /// The rows for `date`, ordered by docket: each in-service unit on the page, if there is one,
    /// and EIA's numbers for every other in-service unit EIA reports that day.
    ///
    /// # Errors
    ///
    /// On an unknown unit name or tag under [`Unknowns::Fail`], and when a unit listed twice has
    /// copies no rule picks between.
    pub fn day(&mut self, date: NaiveDate, page: Option<&StatusPage>) -> Res<Vec<UnitDay>> {
        let mut days = Vec::new();
        let mut reported: HashSet<&'a str> = HashSet::new();
        if let Some(StatusPage::Report(rows)) = page {
            let mut current = HashMap::with_capacity(rows.len());
            for (unit, copies) in self.by_unit(date, rows)? {
                let row = self.one_copy(date, unit, copies)?;
                reported.insert(unit.docket.as_str());
                if unit.in_service(date) {
                    days.push(self.reported(date, unit, &row)?);
                }
                current.insert(unit.docket.as_str(), row);
            }
            self.previous = current;
        }
        if let Some(nuc_status) = self.nuc_status {
            for unit in self.catalog.units.all() {
                if reported.contains(unit.docket.as_str()) || !unit.in_service(date) {
                    continue;
                }
                if let Some(day) = nuc_status.day(&unit.eia, date) {
                    self.summary.from_eia += 1;
                    let mut row = UnitDay::numbers(unit, date, day.power, Source::Eia, Some(day.power), Method::NoReport);
                    self.weigh(unit, &mut row, true);
                    days.push(row);
                }
            }
        }
        days.sort_by(|a, b| a.docket.cmp(&b.docket));
        if !days.is_empty() {
            self.summary.days += days.len() as u64;
            self.summary.first.get_or_insert(date);
            self.summary.last = Some(date);
        }
        Ok(days)
    }

    /// What the build did so far.
    #[must_use]
    pub fn finish(self) -> Summary {
        self.summary
    }

    /// The page's rows grouped by unit, in page order.
    fn by_unit<'r>(&mut self, date: NaiveDate, rows: &'r [Row]) -> Res<Vec<(&'a Unit, Vec<&'r Row>)>> {
        let mut grouped: Vec<(&'a Unit, Vec<&'r Row>)> = Vec::with_capacity(rows.len());
        let mut index: HashMap<&str, usize> = HashMap::with_capacity(rows.len());
        for row in rows {
            let Some(unit) = self.catalog.units.by_nrc_name(&row.unit) else {
                match self.unknowns {
                    Unknowns::Fail => bail!("{date}: unknown unit {:?}; add it to catalog/units.csv", row.unit),
                    Unknowns::Quarantine => {
                        *self.summary.unknown_units.entry(row.unit.clone()).or_default() += 1;
                        continue;
                    }
                }
            };
            match index.entry(unit.docket.as_str()) {
                Entry::Occupied(slot) => grouped[*slot.get()].1.push(row),
                Entry::Vacant(slot) => {
                    slot.insert(grouped.len());
                    grouped.push((unit, vec![row]));
                }
            }
        }
        Ok(grouped)
    }

    /// The one row a unit gets. Copies that differ only in the change mark merge, keeping any mark.
    /// Otherwise a copy identical to the unit's row on the previous published page is a stale
    /// repeat and drops out: on 2013-07-19, one copy of each unit repeats the 2013-07-17 report
    /// (PRD-0002.log.md, T-007).
    fn one_copy(&mut self, date: NaiveDate, unit: &Unit, mut copies: Vec<&Row>) -> Res<Row> {
        if copies.len() > 1 {
            self.summary.duplicates += 1;
            if !all_same(&copies) {
                let previous = self.previous.get(unit.docket.as_str());
                copies.retain(|copy| previous.is_none_or(|previous| !repeats(copy, previous)));
                ensure!(
                    !copies.is_empty() && all_same(&copies),
                    "{date}: {} is listed more than once with different reports, and none repeats the previous page",
                    unit.name
                );
            }
        }
        let mut row = (*copies.first().context("a unit has at least one row")?).clone();
        if let Some(detail) = &mut row.detail {
            detail.changed = copies.iter().any(|copy| copy.detail.as_ref().is_some_and(|detail| detail.changed));
        }
        Ok(row)
    }

    /// A unit's row from the page, observed and attributed.
    fn reported(&mut self, date: NaiveDate, unit: &'a Unit, row: &Row) -> Res<UnitDay> {
        let eia_power = self.nuc_status.and_then(|nuc_status| nuc_status.day(&unit.eia, date)).map(|day| day.power);
        if let Some(eia_power) = eia_power {
            self.summary.compared += 1;
            self.summary.disagreed += u64::from(eia_power != row.power);
        }
        let Some(detail) = &row.detail else {
            self.summary.pending += 1;
            let mut pending = UnitDay::numbers(unit, date, row.power, Source::NrcPage, eia_power, Method::ReasonPending);
            self.weigh(unit, &mut pending, true);
            return Ok(pending);
        };
        let observed = attribution::observe(&self.catalog.tags, detail.reason.as_deref().unwrap_or_default());
        let attributed = if observed.unknown.is_empty() {
            self.attributors.entry(unit.docket.as_str()).or_default().attribute(date, &observed)
        } else {
            match self.unknowns {
                Unknowns::Fail => bail!("{date}, {}: tags nothing knows: {:?}; add a key or rule to catalog/tags.toml", unit.name, observed.unknown),
                Unknowns::Quarantine => {
                    for tag in &observed.unknown {
                        *self.summary.unknown_tags.entry(tag.clone()).or_default() += 1;
                    }
                    Attributed::unattributed(Method::UnknownTag, Evidence::Unknown(observed.unknown.clone()))
                }
            }
        };
        let lost_output = attributed.cause.is_none_or(|cause| cause.lost_output);
        let mut reported = UnitDay {
            docket: unit.docket.as_str().to_owned(),
            date,
            power: row.power,
            source: Source::NrcPage,
            eia_power,
            reason: detail.reason.clone(),
            down: detail.down,
            changed: detail.changed,
            scrams: detail.scrams,
            causes: observed
                .causes
                .iter()
                .map(|(cause, tag)| ObservedCause {
                    cause: cause.name.clone(),
                    tag: tag.clone(),
                })
                .collect(),
            flags: observed.flags.iter().map(|&flag| flag.to_owned()).collect(),
            transition: observed.transition,
            xrefs: observed.xrefs.clone(),
            unclassified: observed.unclassified.clone(),
            family: attributed.family(),
            cause: attributed.cause.map(|cause| cause.name.clone()),
            method: attributed.method,
            evidence: attributed.evidence,
            unknown: observed.unknown,
            capacity_kw: None,
            lost_kwh: None,
        };
        self.weigh(unit, &mut reported, lost_output);
        Ok(reported)
    }

    /// Fills in `row`'s capacity and lost energy, which is zero when `lost_output` is false.
    fn weigh(&mut self, unit: &Unit, row: &mut UnitDay, lost_output: bool) {
        row.capacity_kw = capacity::kw(&self.catalog.capacity, self.nuc_status, unit, row.date);
        row.lost_kwh = row.capacity_kw.map(|capacity_kw| if lost_output { lost_kwh(row.power, capacity_kw) } else { 0 });
        match row.lost_kwh {
            Some(lost) => self.summary.lost_kwh += lost,
            None => self.summary.without_capacity += u64::from(row.power < 100),
        }
    }
}

/// Whether every copy reports the same thing, the change mark aside.
fn all_same(copies: &[&Row]) -> bool {
    copies.windows(2).all(|pair| {
        let (a, b) = (pair[0], pair[1]);
        a.power == b.power
            && match (&a.detail, &b.detail) {
                (Some(a), Some(b)) => (a.down, &a.reason, a.scrams) == (b.down, &b.reason, b.scrams),
                (a, b) => a.is_none() && b.is_none(),
            }
    })
}

/// Whether `copy` repeats `previous` exactly, the unit's spelling aside.
fn repeats(copy: &Row, previous: &Row) -> bool {
    copy.power == previous.power && copy.detail == previous.detail
}

/// Builds the ledger from the newest capture of every status page and the newest `NUC_STATUS`,
/// handing `emit` each day's rows from the first page through the last day either source covers.
///
/// # Errors
///
/// When the raw store can't be read, a page or the `NUC_STATUS` doesn't parse, a day fails (see
/// [`Builder::day`]), or `emit` fails. Under [`Unknowns::Fail`], also when a `NUC_STATUS` generator
/// is missing from the catalog.
pub fn build(store: &RawStore, catalog: &Catalog, unknowns: Unknowns, mut emit: impl FnMut(Vec<UnitDay>) -> Void) -> Res<Summary> {
    let pages = store.latest(Series::PowerStatus)?;
    let nuc_status = match store.latest(Series::NucStatus)?.into_values().next_back() {
        Some(record) => {
            let hash = record.sha256.context("a stored NUC_STATUS has a hash")?;
            let nuc_status = NucStatus::parse(&store.get(&hash)?).with_context(|| format!("the NUC_STATUS fetched {}", record.date))?;
            if unknowns == Unknowns::Fail {
                nuc_status.check(&catalog.units)?;
            }
            Some(nuc_status)
        }
        None => None,
    };
    let Some(&first) = pages.keys().next() else {
        return Ok(Summary::default());
    };
    let last = pages.keys().next_back().copied().into_iter().chain(nuc_status.as_ref().and_then(NucStatus::end)).max().unwrap_or(first);
    let mut builder = Builder::new(catalog, nuc_status.as_ref(), unknowns);
    for date in first.iter_days().take_while(|date| *date <= last) {
        let page = match pages.get(&date).and_then(|record| record.sha256.as_ref()) {
            Some(hash) => Some(status::parse(&store.get(hash)?).with_context(|| format!("the status page for {date}"))?),
            None => None,
        };
        emit(builder.day(date, page.as_ref())?)?;
    }
    Ok(builder.finish())
}
