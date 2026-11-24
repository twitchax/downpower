//! The ledger in Surreal (DESIGN.md Decision 15). Each build is its own table, `unit_day_<n>`, one
//! record per unit-day keyed `[docket, date]`, beside `event_<n>`, one record per Event
//! Notification keyed by its number, and `event_unit_<n>`, one record per unit of a reactor event
//! keyed `[number, unit]`. `ledger:current` names the table the site reads along with
//! the `derived_version` that built it. A rebuild writes a fresh table while the old one
//! keeps serving, flips `ledger:current` and the `unit` table in one transaction, then deletes the
//! old table a unit at a time: one `DELETE` of a million rows peaks at 2.1 GB, and a `REMOVE TABLE`
//! at 900 MB, against about 250 MB for the chunks (PRD-0002.log.md, T-008).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context as _, bail, ensure};
use chrono::{DateTime, Months, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use surrealdb::types::{Bytes, SerdeWrapper};

use downpower_core::barcode::{self, Barcode, Layer, Packer};
use downpower_core::ffd::Series as FfdSeries;
use downpower_core::series::Monthly;
use downpower_core::trends::Trends;
use downpower_core::triggers::Series as ScramSeries;
use downpower_ingest::cannabis::Laws;
use downpower_ingest::chunks::{self, Chunk, Chunker, EventHashes, Hashes};
use downpower_ingest::episodes;
use downpower_ingest::events::{Archive, Event, EventSet};
use downpower_ingest::facilities::{self, EventUnit, Identity};
use downpower_ingest::ffd::{self, Ffd};
use downpower_ingest::ledger::{self, Catalog, Summary, UnitDay, Unknowns};
use downpower_ingest::raw::RawStore;
use downpower_ingest::trends::Collector;
use downpower_ingest::triggers::Triggers;
use downpower_ingest::units::{Unit, Units};

use crate::db::Store;

/// Rows per `INSERT`: big enough to amortize a statement, small enough to keep each transaction
/// in the memtable.
const BATCH: usize = 5_000;

/// Events per `INSERT`: they carry their text, about 1.5 kB each.
const EVENT_BATCH: usize = 500;

/// Batches the builder may run ahead of the writer.
const IN_FLIGHT: usize = 2;

const TABLE_PREFIX: &str = "unit_day_";

/// A build's events live in `event_<n>`, beside its `unit_day_<n>`.
const EVENT_PREFIX: &str = "event_";

/// And its reactor events' units in `event_unit_<n>`, a name `event_` also prefixes.
const EVENT_UNIT_PREFIX: &str = "event_unit_";

/// A finished build, as `ledger:current` records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// The [`ledger::derived_version`] that built it.
    pub version: String,
    /// The table holding its rows.
    pub table: String,
    pub built_at: DateTime<Utc>,
    /// The raw store's [`RawStore::digest`] when the build started: a store whose digest has moved
    /// since holds pages the build hasn't read.
    pub raw_digest: String,
    pub summary: Summary,
    /// What the stored event pages held (PRD-0003 T-003).
    pub events: Archive,
}

/// A row as stored: the unit-day under its `[docket, date]` id.
#[derive(Serialize, Deserialize)]
struct Keyed {
    id: (String, NaiveDate),
    #[serde(flatten)]
    day: UnitDay,
}

/// An event as a build stores it: the event as its page shows it, and who it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent {
    #[serde(flatten)]
    pub event: Event,
    /// `None` for a Part 21 vendor, a licensee with no license number, or a quarantined facility.
    pub identity: Option<Identity>,
    /// The date of the page the event came from; `None` in a build from before PRD-0004 T-005.
    #[serde(default)]
    pub page: Option<NaiveDate>,
}

/// An event as stored, under its number.
#[derive(Serialize, Deserialize)]
struct KeyedEvent {
    id: u32,
    #[serde(flatten)]
    stored: StoredEvent,
}

/// A reactor event's unit as stored, under `[number, unit]`.
#[derive(Serialize, Deserialize)]
struct KeyedUnit {
    id: (u32, u8),
    #[serde(flatten)]
    row: EventUnit,
}

/// An event's rows: its own and one per unit of a reactor event.
struct EventRows {
    event: KeyedEvent,
    units: Vec<KeyedUnit>,
}

/// A `unit` record, keyed by docket.
#[derive(Serialize, Deserialize)]
struct UnitRecord {
    id: String,
    name: String,
    nrc_names: Vec<String>,
    eia_plant: u32,
    eia_generator: String,
    commercial: Option<NaiveDate>,
    shutdown: Option<NaiveDate>,
    restart: Option<NaiveDate>,
}

impl From<&Unit> for UnitRecord {
    fn from(unit: &Unit) -> Self {
        Self {
            id: unit.docket.as_str().to_owned(),
            name: unit.name.clone(),
            nrc_names: unit.nrc_names.clone(),
            eia_plant: unit.eia.plant,
            eia_generator: unit.eia.generator.clone(),
            commercial: unit.commercial,
            shutdown: unit.shutdown,
            restart: unit.restart,
        }
    }
}

/// A build's barcode in one layer, as the `barcode` table stores it under `[table, layer]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packed {
    /// The encoded [`Barcode`].
    pub bytes: Bytes,
    /// The first 16 hex digits of the bytes' sha256: unchanged data keeps its tag across rebuilds.
    pub etag: String,
}

impl Packed {
    fn new(barcode: &Barcode) -> Self {
        let bytes = barcode.encode();
        let etag = hex::encode(&Sha256::digest(&bytes)[..8]);
        Self { bytes: bytes.into(), etag }
    }
}

/// Per layer, a barcode packer and the monthly lost energy, with each unit's barcode row. Rows run
/// in docket order, as the buffer's format promises (Decision 23).
struct Packers {
    rows: HashMap<String, usize>,
    layers: Vec<(Layer, Packer, Monthly)>,
}

impl Packers {
    fn new(units: &Units) -> Self {
        let mut units: Vec<barcode::Unit> = units
            .all()
            .iter()
            .map(|unit| barcode::Unit {
                docket: unit.docket.as_str().to_owned(),
                name: unit.name.clone(),
            })
            .collect();
        units.sort_by(|a, b| a.docket.cmp(&b.docket));
        let rows = units.iter().enumerate().map(|(row, unit)| (unit.docket.clone(), row)).collect();
        let layers = Layer::ALL.into_iter().map(|layer| (layer, Packer::new(units.clone()), Monthly::default())).collect();
        Self { rows, layers }
    }

    fn push(&mut self, day: &UnitDay) -> anyhow::Result<()> {
        let row = *self.rows.get(&day.docket).with_context(|| format!("no barcode row for {}", day.docket))?;
        for (layer, packer, monthly) in &mut self.layers {
            let cell = day.cell(*layer);
            packer.set(row, day.date, cell)?;
            // Every row's month counts, at zero too, so the series spans what the barcode does.
            monthly.add(day.date, cell.family, day.lost_kwh.unwrap_or(0))?;
        }
        Ok(())
    }

    fn finish(self) -> anyhow::Result<Vec<(Layer, Option<Packed>, Monthly)>> {
        let mut views = Vec::new();
        for (layer, packer, monthly) in self.layers {
            views.push((layer, packer.finish()?.as_ref().map(Packed::new), monthly));
        }
        Ok(views)
    }
}

/// Defines the tables every build shares; Surreal refuses to read a table that was never defined.
///
/// # Errors
///
/// When the definitions fail.
pub async fn define(store: &Store) -> anyhow::Result<()> {
    store
        .query("DEFINE TABLE IF NOT EXISTS ledger SCHEMALESS; DEFINE TABLE IF NOT EXISTS unit SCHEMALESS; DEFINE TABLE IF NOT EXISTS barcode SCHEMALESS; DEFINE TABLE IF NOT EXISTS lost SCHEMALESS; DEFINE TABLE IF NOT EXISTS chunks SCHEMALESS; DEFINE TABLE IF NOT EXISTS trends SCHEMALESS; DEFINE TABLE IF NOT EXISTS ffd SCHEMALESS; DEFINE TABLE IF NOT EXISTS scrams SCHEMALESS;")
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("defining the ledger tables")?;
    Ok(())
}

/// Checks that `table` is a build table's name, since it is spliced into queries.
fn checked(table: &str) -> anyhow::Result<&str> {
    let digits = table.strip_prefix(TABLE_PREFIX).unwrap_or_default();
    ensure!(!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()), "{table:?} is not a build table");
    Ok(table)
}

/// The build the site reads, if there is one. A record an older binary wrote in another shape counts
/// as none, so the next refresh replaces it rather than failing on it forever.
///
/// # Errors
///
/// When the query fails.
pub async fn current(store: &Store) -> anyhow::Result<Option<Build>> {
    Ok(record(store).await?.ok().flatten())
}

/// `ledger:current` as a [`Build`], or the error that says it is in another shape. Only a refresh
/// reports that error: `/status` reads the record on every visit while the rebuild runs.
async fn record(store: &Store) -> anyhow::Result<Result<Option<Build>, serde_json::Error>> {
    let record: Option<SerdeWrapper<serde_json::Value>> = store
        .query("SELECT * OMIT id FROM ONLY ledger:current")
        .await
        .context("reading ledger:current")?
        .take(0)
        .context("reading ledger:current")?;
    Ok(record.map_or(Ok(None), |SerdeWrapper(record)| serde_json::from_value(record).map(Some)))
}

/// The table the site reads, whatever shape the rest of `ledger:current` is in. A deploy that
/// changes [`Build`] makes [`current`] read none until its rebuild lands, and the views keep
/// reading the old table meanwhile.
///
/// # Errors
///
/// When the query fails.
pub async fn serving(store: &Store) -> anyhow::Result<Option<String>> {
    store
        .query("SELECT VALUE `table` FROM ONLY ledger:current")
        .await
        .context("reading ledger:current")?
        .take(0)
        .context("reading ledger:current's table")
}

/// What the writer does with rows.
enum Write {
    /// Rows for a fresh table.
    Insert(Vec<Keyed>),
    /// One unit-month's rows, replacing whatever the table holds for that month; none deletes it.
    Replace { docket: String, month: NaiveDate, rows: Vec<Keyed> },
    /// Events and their units for a fresh event table.
    InsertEvents { events: Vec<KeyedEvent>, units: Vec<KeyedUnit> },
    /// One event and its units, replacing the stored ones; `None` deletes them.
    ReplaceEvent { number: u32, rows: Option<Box<EventRows>> },
}

/// Takes each finished unit-month: a fresh build writes every one, an update only those whose hash
/// moved since the build it updates.
struct Sink {
    old: Option<Hashes>,
    hashes: Hashes,
    batch: Vec<Keyed>,
    sender: tokio::sync::mpsc::Sender<Write>,
    replaced: u64,
}

fn keyed(days: Vec<UnitDay>) -> impl Iterator<Item = Keyed> {
    days.into_iter().map(|day| Keyed { id: (day.docket.clone(), day.date), day })
}

impl Sink {
    fn send(&self, write: Write) -> anyhow::Result<()> {
        self.sender.blocking_send(write).context("the ledger writer stopped")
    }

    fn take(&mut self, chunk: Chunk) -> anyhow::Result<()> {
        let key = (chunk.docket, chunk.month);
        let unchanged = self.old.as_ref().is_some_and(|old| old.0.get(&key) == Some(&chunk.hash));
        self.hashes.0.insert(key.clone(), chunk.hash);
        if self.old.is_none() {
            self.batch.extend(keyed(chunk.days));
            if self.batch.len() >= BATCH {
                let batch = std::mem::replace(&mut self.batch, Vec::with_capacity(BATCH));
                self.send(Write::Insert(batch))?;
            }
        } else if !unchanged {
            self.replaced += 1;
            let (docket, month) = key;
            self.send(Write::Replace {
                docket,
                month,
                rows: keyed(chunk.days).collect(),
            })?;
        }
        Ok(())
    }

    /// Flushes the last batch and, for an update, deletes the months the old build had and this one
    /// doesn't. Returns the new hashes and how many months an update replaced.
    fn finish(mut self) -> anyhow::Result<(Hashes, u64)> {
        if !self.batch.is_empty() {
            let batch = std::mem::take(&mut self.batch);
            self.send(Write::Insert(batch))?;
        }
        if let Some(old) = &self.old {
            for (docket, month) in old.0.keys().filter(|key| !self.hashes.0.contains_key(*key)) {
                self.replaced += 1;
                self.send(Write::Replace {
                    docket: docket.clone(),
                    month: *month,
                    rows: Vec::new(),
                })?;
            }
        }
        Ok((self.hashes, self.replaced))
    }
}

/// Sends `set`'s events to the writer: every one for a fresh table, otherwise only those whose
/// hash moved since `old`, and a deletion for each one gone. Returns the new hashes and how many
/// events an update rewrote or deleted.
fn send_events(set: &EventSet, old: Option<&EventHashes>, sender: &tokio::sync::mpsc::Sender<Write>) -> anyhow::Result<(EventHashes, u64)> {
    let send = |write: Write| sender.blocking_send(write).context("the ledger writer stopped");
    let mut hashes = EventHashes::default();
    let (mut events, mut units) = (Vec::with_capacity(EVENT_BATCH), Vec::new());
    let mut moved = 0;
    for (number, event) in &set.events {
        let identity = set.identities.get(number);
        let page = set.pages.get(number).copied();
        // The unit rows follow from the event and its identity, so this hash covers them too.
        let hash = chunks::hash(&(event, identity, page));
        hashes.0.insert(*number, hash);
        let rows = || EventRows {
            event: KeyedEvent {
                id: *number,
                stored: StoredEvent {
                    event: event.clone(),
                    identity: identity.cloned(),
                    page,
                },
            },
            units: facilities::event_units(event, identity).into_iter().map(|row| KeyedUnit { id: (row.event, row.unit), row }).collect(),
        };
        match old {
            None => {
                let EventRows { event, units: rows } = rows();
                events.push(event);
                units.extend(rows);
                if events.len() >= EVENT_BATCH {
                    send(Write::InsertEvents {
                        events: std::mem::replace(&mut events, Vec::with_capacity(EVENT_BATCH)),
                        units: std::mem::take(&mut units),
                    })?;
                }
            }
            Some(old) if old.0.get(number) != Some(&hash) => {
                moved += 1;
                send(Write::ReplaceEvent {
                    number: *number,
                    rows: Some(Box::new(rows())),
                })?;
            }
            Some(_) => {}
        }
    }
    if !events.is_empty() {
        send(Write::InsertEvents { events, units })?;
    }
    if let Some(old) = old {
        for number in old.0.keys().filter(|number| !hashes.0.contains_key(number)) {
            moved += 1;
            send(Write::ReplaceEvent { number: *number, rows: None })?;
        }
    }
    Ok((hashes, moved))
}

/// A build's hashes: the unit-months' and the events'.
struct Old {
    days: Hashes,
    events: EventHashes,
}

/// What a build wrote beside its rows.
struct Written {
    summary: Summary,
    events: Archive,
    raw_digest: String,
    months: usize,
    replaced: u64,
    events_moved: u64,
}

/// Builds the event set and the ledger from the raw store under `data_dir` into `table` and its
/// event table: every row and event when `old` is `None`, otherwise only the unit-months and events
/// whose hash differs from `old`. Then stores the views and the hashes under the tables' names. The
/// events go first, so the ledger can resolve references against them (T-005).
async fn write(store: &Store, data_dir: &Path, unknowns: Unknowns, table: &str, old: Option<Old>) -> anyhow::Result<Written> {
    let event_table = event_table(table)?;
    let unit_table = event_unit_table(table)?;
    let units = Units::catalog()?;
    let raw = RawStore::open(data_dir)?;
    let raw_digest = raw.digest()?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Write>(IN_FLIGHT);
    let mut packers = Packers::new(&units);
    let builder = tokio::task::spawn_blocking(move || {
        let catalog = Catalog::embedded()?;
        let (old_days, old_events) = old.map_or((None, None), |old| (Some(old.days), Some(old.events)));
        let set = EventSet::build(&raw, &catalog.facilities, unknowns)?;
        let (event_hashes, events_moved) = send_events(&set, old_events.as_ref(), &sender)?;
        let mut trends = Collector::new(&set, &catalog.units);
        let (rules, laws) = (Ffd::catalog()?, Laws::catalog()?);
        let mut fitness = ffd::Collector::new(&set, &catalog.units, &rules, &laws);
        let triggers = Triggers::catalog()?;
        triggers.check(&set)?;
        let mut scrams = episodes::Collector::new(&set, &catalog.units, |event| triggers.tags(event));
        let mut sink = Sink {
            old: old_days,
            hashes: Hashes::default(),
            batch: Vec::with_capacity(BATCH),
            sender,
            replaced: 0,
        };
        let mut chunker = Chunker::default();
        let summary = ledger::build(&raw, &catalog, &set, unknowns, |days| {
            for day in &days {
                packers.push(day)?;
            }
            trends.push(&days)?;
            fitness.push(&days);
            scrams.push(&days);
            chunker.push(days).into_iter().try_for_each(|chunk| sink.take(chunk))
        })?;
        chunker.finish().into_iter().try_for_each(|chunk| sink.take(chunk))?;
        let (hashes, replaced) = sink.finish()?;
        let views = Views {
            layers: packers.finish()?,
            trends: trends.finish(),
            ffd: fitness.finish(),
            scrams: scrams.finish(),
        };
        anyhow::Ok((summary, set.archive, views, hashes, replaced, event_hashes, events_moved))
    });
    let insert = format!("INSERT INTO {table} $rows RETURN NONE");
    let replace = format!("BEGIN; DELETE {table}:[$docket, $from]..[$docket, $to] RETURN NONE; INSERT INTO {table} $rows RETURN NONE; COMMIT;");
    let insert_events = format!("INSERT INTO {event_table} $events RETURN NONE; INSERT INTO {unit_table} $units RETURN NONE;");
    while let Some(write) = receiver.recv().await {
        let query = match write {
            Write::InsertEvents { events, units } => store.query(insert_events.as_str()).bind(("events", SerdeWrapper(events))).bind(("units", SerdeWrapper(units))),
            Write::ReplaceEvent { number, rows } => {
                let (events, units) = rows.map_or_else(|| (Vec::new(), Vec::new()), |rows| (vec![rows.event], rows.units));
                // The number is a u32, so splicing it in is safe.
                store
                    .query(format!(
                        "BEGIN; DELETE {event_table}:{number} RETURN NONE; DELETE {unit_table}:[{number}, NONE]..=[{number}, ..] RETURN NONE; INSERT INTO {event_table} $events RETURN NONE; INSERT INTO {unit_table} $units RETURN NONE; COMMIT;"
                    ))
                    .bind(("events", SerdeWrapper(events)))
                    .bind(("units", SerdeWrapper(units)))
            }
            Write::Insert(rows) => store.query(insert.as_str()).bind(("rows", SerdeWrapper(rows))),
            Write::Replace { docket, month, rows } => store
                .query(replace.as_str())
                .bind(("docket", docket))
                .bind(("from", month.to_string()))
                .bind(("to", month.checked_add_months(Months::new(1)).unwrap_or(NaiveDate::MAX).to_string()))
                .bind(("rows", SerdeWrapper(rows))),
        };
        query.await.and_then(surrealdb::IndexedResults::check).with_context(|| format!("writing to {table}"))?;
    }
    let (summary, events, views, hashes, replaced, event_hashes, events_moved) = builder.await.context("the ledger builder panicked")??;
    store_views(store, table, views, &hashes).await?;
    store_bytes(store, &event_table, event_hashes.encode()).await.context("storing the event hashes")?;
    Ok(Written {
        summary,
        events,
        raw_digest,
        months: hashes.0.len(),
        replaced,
        events_moved,
    })
}

/// What a build stores beside its rows: per layer its barcode and monthly series, the fleet trends
/// and the FFD series.
struct Views {
    layers: Vec<(Layer, Option<Packed>, Monthly)>,
    trends: Trends,
    ffd: FfdSeries,
    scrams: ScramSeries,
}

/// Stores a build's barcodes, monthly series, trends and chunk hashes under its table's name, so
/// they swap in with it.
async fn store_views(store: &Store, table: &str, views: Views, hashes: &Hashes) -> anyhow::Result<()> {
    store
        .query("UPSERT trends:[$table] CONTENT $trends RETURN NONE")
        .bind(("table", table.to_owned()))
        .bind(("trends", SerdeWrapper(views.trends)))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("storing the trends")?;
    store
        .query("UPSERT ffd:[$table] CONTENT $ffd RETURN NONE")
        .bind(("table", table.to_owned()))
        .bind(("ffd", SerdeWrapper(views.ffd)))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("storing the FFD series")?;
    store
        .query("UPSERT scrams:[$table] CONTENT $scrams RETURN NONE")
        .bind(("table", table.to_owned()))
        .bind(("scrams", SerdeWrapper(views.scrams)))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("storing the scram series")?;
    for (layer, packed, monthly) in views.layers {
        if let Some(packed) = packed {
            store
                .query("UPSERT barcode:[$table, $layer] CONTENT $packed RETURN NONE")
                .bind(("table", table.to_owned()))
                .bind(("layer", layer.slug()))
                .bind(("packed", SerdeWrapper(packed)))
                .await
                .and_then(surrealdb::IndexedResults::check)
                .with_context(|| format!("storing the {} barcode", layer.slug()))?;
        }
        store
            .query("UPSERT lost:[$table, $layer] CONTENT $monthly RETURN NONE")
            .bind(("table", table.to_owned()))
            .bind(("layer", layer.slug()))
            .bind(("monthly", SerdeWrapper(monthly)))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .with_context(|| format!("storing the {} monthly series", layer.slug()))?;
    }
    store_bytes(store, table, hashes.encode()).await.context("storing the chunk hashes")
}

/// Stores a table's hashes in `chunks`, under the table's name.
async fn store_bytes(store: &Store, table: &str, bytes: Vec<u8>) -> anyhow::Result<()> {
    store
        .query("UPSERT chunks:[$table] CONTENT { bytes: $bytes } RETURN NONE")
        .bind(("table", table.to_owned()))
        .bind(("bytes", Bytes::from(bytes)))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .with_context(|| format!("storing the hashes of {table}"))?;
    Ok(())
}

/// The hashes stored under a table's name, if its build stored them.
async fn stored_bytes(store: &Store, table: &str) -> anyhow::Result<Option<Bytes>> {
    store
        .query("SELECT VALUE bytes FROM ONLY chunks:[$table]")
        .bind(("table", table.to_owned()))
        .await
        .with_context(|| format!("reading the hashes of {table}"))?
        .take(0)
        .with_context(|| format!("reading the hashes of {table}"))
}

/// The unit-month and event hashes of the build in `table`, if it stored both.
async fn hashes(store: &Store, table: &str) -> anyhow::Result<Option<Old>> {
    let (Some(days), Some(events)) = (stored_bytes(store, table).await?, stored_bytes(store, &event_table(table)?).await?) else {
        return Ok(None);
    };
    Ok(Some(Old {
        days: Hashes::decode(&days)?,
        events: EventHashes::decode(&events)?,
    }))
}

/// Builds the ledger from the raw store under `data_dir` into a fresh table, makes it current, and
/// drops every other build table.
///
/// # Errors
///
/// When the build fails (see [`ledger::build`]) or a write fails. The previous build stays
/// current, and the next rebuild drops the partial table.
pub async fn rebuild(store: &Store, data_dir: &Path, unknowns: Unknowns) -> anyhow::Result<Build> {
    let started = Instant::now();
    let units = Units::catalog()?;
    let table = format!("{TABLE_PREFIX}{}", Utc::now().timestamp_micros());
    let event_table = event_table(&table)?;
    let unit_table = event_unit_table(&table)?;
    // A build that died part way left its tables behind; the current one keeps serving.
    let serving = serving(store).await?;
    drop_tables(store, &units, serving.as_deref()).await?;
    store
        .query(format!("DEFINE TABLE {table} SCHEMALESS; DEFINE TABLE {event_table} SCHEMALESS; DEFINE TABLE {unit_table} SCHEMALESS;"))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .with_context(|| format!("defining {table}, {event_table} and {unit_table}"))?;
    tracing::info!(%table, "rebuilding the ledger");

    let written = write(store, data_dir, unknowns, &table, None).await?;
    let build = Build {
        version: ledger::derived_version(),
        table,
        built_at: Utc::now(),
        raw_digest: written.raw_digest,
        summary: written.summary,
        events: written.events,
    };
    let unit_records: Vec<UnitRecord> = units.all().iter().map(UnitRecord::from).collect();
    store
        .query("BEGIN; DELETE unit; INSERT INTO unit $units RETURN NONE; UPSERT ledger:current CONTENT $build RETURN NONE; COMMIT;")
        .bind(("units", SerdeWrapper(unit_records)))
        .bind(("build", SerdeWrapper(build.clone())))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("making the new build current")?;
    drop_tables(store, &units, Some(&build.table)).await?;
    tracing::info!(
        table = %build.table,
        days = build.summary.days,
        months = written.months,
        events = build.events.events,
        elapsed_secs = started.elapsed().as_secs(),
        "ledger rebuilt"
    );
    Ok(build)
}

/// Updates `build`'s tables in place from the raw store under `data_dir`: the whole ledger and
/// event set are built again, but only the unit-months and events that differ from the stored
/// hashes are written. The same derived
/// version means the same parser and catalog, so the months a few new pages touch are all that
/// moves.
///
/// # Errors
///
/// When the build or a write fails. The months written so far stay; `ledger:current` keeps the old
/// digest, so the next refresh compares against the old hashes and writes them again.
pub async fn update(store: &Store, data_dir: &Path, unknowns: Unknowns, build: Build) -> anyhow::Result<Build> {
    let started = Instant::now();
    let old = hashes(store, &build.table).await?.with_context(|| format!("{} has no hashes to update against", build.table))?;
    tracing::info!(table = %build.table, "updating the ledger");
    let written = write(store, data_dir, unknowns, &build.table, Some(old)).await?;
    let build = Build {
        built_at: Utc::now(),
        raw_digest: written.raw_digest,
        summary: written.summary,
        events: written.events,
        ..build
    };
    store
        .query("UPSERT ledger:current CONTENT $build RETURN NONE")
        .bind(("build", SerdeWrapper(build.clone())))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("recording the updated build")?;
    tracing::info!(
        table = %build.table,
        months = written.replaced,
        events = written.events_moved,
        elapsed_secs = started.elapsed().as_secs(),
        "ledger updated"
    );
    Ok(build)
}

/// Brings the ledger up to date, and returns the build when anything was written. A deploy that
/// changes the parser or the catalog rebuilds into a fresh table, which serves only once complete;
/// new raw content under the same version updates the current table, rewriting only the unit-months
/// it changes; a refetch that changed nothing does nothing. Unknown tags and units are quarantined
/// (Decision 21).
///
/// # Errors
///
/// When `ledger:current` or the raw manifest can't be read, or the build fails, in which case the
/// previous build stays current.
pub async fn refresh(store: &Store, data_dir: &Path) -> anyhow::Result<Option<Build>> {
    let version = ledger::derived_version();
    let raw_digest = RawStore::open(data_dir)?.digest()?;
    let current = record(store).await?.unwrap_or_else(|error| {
        tracing::warn!(%error, "ledger:current is from another version; rebuilding over it");
        None
    });
    match current {
        Some(build) if build.version == version && build.raw_digest == raw_digest => {
            tracing::info!(%version, "the ledger is current");
            Ok(None)
        }
        Some(build) if build.version == version && hashes(store, &build.table).await?.is_some() => update(store, data_dir, Unknowns::Quarantine, build).await.map(Some),
        _ => rebuild(store, data_dir, Unknowns::Quarantine).await.map(Some),
    }
}

/// Checks that `table` is an event or event unit table's name, since it is spliced into queries.
fn checked_events(table: &str) -> anyhow::Result<&str> {
    let digits = table.strip_prefix(EVENT_UNIT_PREFIX).or_else(|| table.strip_prefix(EVENT_PREFIX)).unwrap_or_default();
    ensure!(!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()), "{table:?} is not an event table");
    Ok(table)
}

/// The tables in the database.
async fn tables(store: &Store) -> anyhow::Result<Vec<String>> {
    let info: Option<SerdeWrapper<serde_json::Value>> = store.query("INFO FOR DB").await.context("listing tables")?.take(0).context("listing tables")?;
    let info = info.context("INFO FOR DB returned nothing")?.0;
    let Some(tables) = info.get("tables").and_then(serde_json::Value::as_object) else {
        bail!("INFO FOR DB has no tables map: {info}");
    };
    Ok(tables.keys().cloned().collect())
}

/// Drops every build's tables except `keep`'s: a build table a unit at a time and then the empty
/// table, an event or event unit table at once. A build's 20,000 events are about 35 MB, so its
/// `REMOVE TABLE` stays small beside the 900 MB a million unit-days takes (PRD-0002.log.md, T-008).
async fn drop_tables(store: &Store, units: &Units, keep: Option<&str>) -> anyhow::Result<()> {
    let keep_events = match keep {
        Some(keep) => vec![event_table(keep)?, event_unit_table(keep)?],
        None => Vec::new(),
    };
    let all = tables(store).await?;
    for event_table in all.iter().filter(|table| table.starts_with(EVENT_PREFIX) && !keep_events.contains(table)) {
        let event_table = checked_events(event_table)?;
        store
            .query(format!("DELETE chunks:[$table] RETURN NONE; REMOVE TABLE {event_table};"))
            .bind(("table", event_table.to_owned()))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .with_context(|| format!("removing {event_table}"))?;
        tracing::info!(%event_table, "dropped an old event table");
    }
    for table in all {
        if !table.starts_with(TABLE_PREFIX) || keep == Some(table.as_str()) {
            continue;
        }
        let table = checked(&table)?;
        let delete = format!("DELETE {table}:[$docket, NONE]..=[$docket, ..] RETURN NONE");
        for unit in units.all() {
            store
                .query(delete.as_str())
                .bind(("docket", unit.docket.as_str().to_owned()))
                .await
                .and_then(surrealdb::IndexedResults::check)
                .with_context(|| format!("clearing {table}"))?;
        }
        store
            .query("DELETE barcode:[$table, NONE]..=[$table, ..] RETURN NONE; DELETE lost:[$table, NONE]..=[$table, ..] RETURN NONE; DELETE trends:[$table] RETURN NONE; DELETE ffd:[$table] RETURN NONE; DELETE scrams:[$table] RETURN NONE; DELETE chunks:[$table] RETURN NONE;")
            .bind(("table", table.to_owned()))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .with_context(|| format!("deleting the views of {table}"))?;
        store
            .query(format!("REMOVE TABLE {table}"))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .with_context(|| format!("removing {table}"))?;
        tracing::info!(%table, "dropped an old ledger build");
    }
    Ok(())
}

/// The event table of the build whose rows are in `table`.
///
/// # Errors
///
/// When `table` isn't a build table.
pub fn event_table(table: &str) -> anyhow::Result<String> {
    let suffix = checked(table)?.strip_prefix(TABLE_PREFIX).unwrap_or_default();
    Ok(format!("{EVENT_PREFIX}{suffix}"))
}

/// The event unit table of the build whose rows are in `table`.
///
/// # Errors
///
/// When `table` isn't a build table.
pub fn event_unit_table(table: &str) -> anyhow::Result<String> {
    let suffix = checked(table)?.strip_prefix(TABLE_PREFIX).unwrap_or_default();
    Ok(format!("{EVENT_UNIT_PREFIX}{suffix}"))
}

/// One event from the build whose rows are in `table`.
///
/// # Errors
///
/// When `table` isn't a build table or the query fails.
pub async fn event(store: &Store, table: &str, number: u32) -> anyhow::Result<Option<StoredEvent>> {
    let event_table = event_table(table)?;
    let event: Option<SerdeWrapper<StoredEvent>> = store
        .query(format!("SELECT * OMIT id FROM ONLY {event_table}:{number}"))
        .await
        .with_context(|| format!("reading event {number} from {event_table}"))?
        .take(0)
        .with_context(|| format!("reading event {number} from {event_table}"))?;
    Ok(event.map(|event| event.0))
}

/// Every event from the build whose rows are in `table`.
///
/// # Errors
///
/// When `table` isn't a build table, or the query or a row fails.
pub async fn events(store: &Store, table: &str) -> anyhow::Result<Vec<StoredEvent>> {
    let event_table = event_table(table)?;
    let events: Vec<SerdeWrapper<StoredEvent>> = store
        .query(format!("SELECT * OMIT id FROM {event_table}"))
        .await
        .with_context(|| format!("reading the events in {event_table}"))?
        .take(0)
        .with_context(|| format!("reading the events in {event_table}"))?;
    Ok(events.into_iter().map(|event| event.0).collect())
}

/// Every reactor event's unit rows from the build whose rows are in `table`, by event and unit.
///
/// # Errors
///
/// When `table` isn't a build table or the query fails.
pub async fn event_units(store: &Store, table: &str) -> anyhow::Result<Vec<EventUnit>> {
    let unit_table = event_unit_table(table)?;
    let rows: Vec<SerdeWrapper<EventUnit>> = store
        .query(format!("SELECT * OMIT id FROM {unit_table} ORDER BY event, unit"))
        .await
        .with_context(|| format!("reading {unit_table}"))?
        .take(0)
        .with_context(|| format!("reading {unit_table}"))?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// One unit's rows in `table`, oldest first.
///
/// # Errors
///
/// When `table` isn't a build table or the query fails.
pub async fn unit_days(store: &Store, table: &str, docket: &str) -> anyhow::Result<Vec<UnitDay>> {
    let table = checked(table)?;
    let rows: Vec<SerdeWrapper<UnitDay>> = store
        .query(format!("SELECT * OMIT id FROM {table}:[$docket, NONE]..=[$docket, ..]"))
        .bind(("docket", docket.to_owned()))
        .await
        .with_context(|| format!("reading {docket} from {table}"))?
        .take(0)
        .with_context(|| format!("reading {docket} from {table}"))?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// `table`'s barcode in `layer`, if it has one.
///
/// # Errors
///
/// When the query fails or the record doesn't parse.
pub async fn barcode(store: &Store, table: &str, layer: Layer) -> anyhow::Result<Option<Packed>> {
    let packed: Option<SerdeWrapper<Packed>> = store
        .query("SELECT bytes, etag FROM ONLY barcode:[$table, $layer]")
        .bind(("table", table.to_owned()))
        .bind(("layer", layer.slug()))
        .await
        .with_context(|| format!("reading the {} barcode of {table}", layer.slug()))?
        .take(0)
        .with_context(|| format!("reading the {} barcode of {table}", layer.slug()))?;
    Ok(packed.map(|packed| packed.0))
}

/// The tag of `table`'s barcode in `layer`, if it has one. It is small enough to read on every
/// request, so a cache keyed by it can't outlive an update that rewrites the barcode in place.
///
/// # Errors
///
/// When the query fails.
pub async fn barcode_etag(store: &Store, table: &str, layer: Layer) -> anyhow::Result<Option<String>> {
    store
        .query("SELECT VALUE etag FROM ONLY barcode:[$table, $layer]")
        .bind(("table", table.to_owned()))
        .bind(("layer", layer.slug()))
        .await
        .with_context(|| format!("reading the {} barcode's tag of {table}", layer.slug()))?
        .take(0)
        .with_context(|| format!("reading the {} barcode's tag of {table}", layer.slug()))
}

/// One unit-day from `table`.
///
/// # Errors
///
/// When `table` isn't a build table or the query fails.
pub async fn unit_day(store: &Store, table: &str, docket: &str, date: NaiveDate) -> anyhow::Result<Option<UnitDay>> {
    let table = checked(table)?;
    let row: Option<SerdeWrapper<UnitDay>> = store
        .query(format!("SELECT * OMIT id FROM ONLY {table}:[$docket, $date]"))
        .bind(("docket", docket.to_owned()))
        .bind(("date", date.to_string()))
        .await
        .with_context(|| format!("reading {docket} on {date} from {table}"))?
        .take(0)
        .with_context(|| format!("reading {docket} on {date} from {table}"))?;
    Ok(row.map(|row| row.0))
}

/// `table`'s monthly lost energy in `layer`, if it has one.
///
/// # Errors
///
/// When the query fails or the record doesn't parse.
pub async fn monthly(store: &Store, table: &str, layer: Layer) -> anyhow::Result<Option<Monthly>> {
    let monthly: Option<SerdeWrapper<Monthly>> = store
        .query("SELECT first, lost_kwh FROM ONLY lost:[$table, $layer]")
        .bind(("table", table.to_owned()))
        .bind(("layer", layer.slug()))
        .await
        .with_context(|| format!("reading the {} monthly series of {table}", layer.slug()))?
        .take(0)
        .with_context(|| format!("reading the {} monthly series of {table}", layer.slug()))?;
    Ok(monthly.map(|monthly| monthly.0))
}

/// `table`'s fleet trends, if its build stored them (PRD-0006).
///
/// # Errors
///
/// When the query fails or the record doesn't parse.
pub async fn trends(store: &Store, table: &str) -> anyhow::Result<Option<Trends>> {
    let trends: Option<SerdeWrapper<Trends>> = store
        .query("SELECT first, last, months, outages FROM ONLY trends:[$table]")
        .bind(("table", table.to_owned()))
        .await
        .with_context(|| format!("reading the trends of {table}"))?
        .take(0)
        .with_context(|| format!("reading the trends of {table}"))?;
    Ok(trends.map(|trends| trends.0))
}

/// `table`'s FFD series, if its build stored one (PRD-0005 T-006).
///
/// # Errors
///
/// When the query fails or the record doesn't parse.
pub async fn ffd(store: &Store, table: &str) -> anyhow::Result<Option<FfdSeries>> {
    let series: Option<SerdeWrapper<FfdSeries>> = store
        .query("SELECT first, last, events, exposure, laws FROM ONLY ffd:[$table]")
        .bind(("table", table.to_owned()))
        .await
        .with_context(|| format!("reading the FFD series of {table}"))?
        .take(0)
        .with_context(|| format!("reading the FFD series of {table}"))?;
    Ok(series.map(|series| series.0))
}

/// `table`'s scram series, if its build stored one (PRD-0007 T-005).
///
/// # Errors
///
/// When the query fails or the record doesn't parse.
pub async fn scrams(store: &Store, table: &str) -> anyhow::Result<Option<ScramSeries>> {
    let series: Option<SerdeWrapper<ScramSeries>> = store
        .query("SELECT first, last, scrams, units FROM ONLY scrams:[$table]")
        .bind(("table", table.to_owned()))
        .await
        .with_context(|| format!("reading the scram series of {table}"))?
        .take(0)
        .with_context(|| format!("reading the scram series of {table}"))?;
    Ok(series.map(|series| series.0))
}
