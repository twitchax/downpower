//! The ledger in Surreal (DESIGN.md Decision 15). Each build is its own table, `unit_day_<n>`, one
//! record per unit-day keyed `[docket, date]`, and `ledger:current` names the table the site reads
//! along with the `derived_version` that built it. A rebuild writes a fresh table while the old one
//! keeps serving, flips `ledger:current` and the `unit` table in one transaction, then deletes the
//! old table a unit at a time: one `DELETE` of a million rows peaks at 2.1 GB, and a `REMOVE TABLE`
//! at 900 MB, against about 250 MB for the chunks (PRD-0002.log.md, T-008).

use std::path::Path;
use std::time::Instant;

use anyhow::{Context as _, bail, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::types::SerdeWrapper;

use downpower_ingest::ledger::{self, Summary, UnitDay, Unknowns};
use downpower_ingest::raw::RawStore;
use downpower_ingest::tags::Tags;
use downpower_ingest::units::{Unit, Units};

use crate::db::Store;

/// Rows per `INSERT`: big enough to amortize a statement, small enough to keep each transaction
/// in the memtable.
const BATCH: usize = 5_000;

/// Batches the builder may run ahead of the writer.
const IN_FLIGHT: usize = 2;

const TABLE_PREFIX: &str = "unit_day_";

/// A finished build, as `ledger:current` records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// The [`ledger::derived_version`] that built it.
    pub version: String,
    /// The table holding its rows.
    pub table: String,
    pub built_at: DateTime<Utc>,
    pub summary: Summary,
}

/// A row as stored: the unit-day under its `[docket, date]` id.
#[derive(Serialize, Deserialize)]
struct Keyed {
    id: (String, NaiveDate),
    #[serde(flatten)]
    day: UnitDay,
}

/// A `unit` record, keyed by docket.
#[derive(Serialize, Deserialize)]
struct UnitRecord {
    id: String,
    name: String,
    nrc_names: Vec<String>,
    eia_plant: u32,
    eia_generator: String,
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
            shutdown: unit.shutdown,
            restart: unit.restart,
        }
    }
}

/// Defines the tables every build shares; Surreal refuses to read a table that was never defined.
///
/// # Errors
///
/// When the definitions fail.
pub async fn define(store: &Store) -> anyhow::Result<()> {
    store
        .query("DEFINE TABLE IF NOT EXISTS ledger SCHEMALESS; DEFINE TABLE IF NOT EXISTS unit SCHEMALESS;")
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

/// The build the site reads, if there is one.
///
/// # Errors
///
/// When the query fails or the record doesn't parse.
pub async fn current(store: &Store) -> anyhow::Result<Option<Build>> {
    let build: Option<SerdeWrapper<Build>> = store
        .query("SELECT * OMIT id FROM ONLY ledger:current")
        .await
        .context("reading ledger:current")?
        .take(0)
        .context("reading ledger:current")?;
    Ok(build.map(|build| build.0))
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
    // A build that died part way left its table behind; the current one keeps serving.
    let serving = current(store).await?.map(|build| build.table);
    drop_tables(store, &units, serving.as_deref()).await?;
    store
        .query(format!("DEFINE TABLE {table} SCHEMALESS"))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .with_context(|| format!("defining {table}"))?;
    tracing::info!(%table, "rebuilding the ledger");

    let raw = RawStore::open(data_dir)?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Vec<Keyed>>(IN_FLIGHT);
    let builder = tokio::task::spawn_blocking(move || {
        let tags = Tags::catalog()?;
        let units = Units::catalog()?;
        let mut batch = Vec::with_capacity(BATCH);
        let summary = ledger::build(&raw, &tags, &units, unknowns, |days| {
            batch.extend(days.into_iter().map(|day| Keyed { id: (day.docket.clone(), day.date), day }));
            if batch.len() >= BATCH {
                sender.blocking_send(std::mem::replace(&mut batch, Vec::with_capacity(BATCH))).context("the ledger writer stopped")?;
            }
            Ok(())
        })?;
        if !batch.is_empty() {
            sender.blocking_send(batch).context("the ledger writer stopped")?;
        }
        anyhow::Ok(summary)
    });
    let insert = format!("INSERT INTO {table} $rows RETURN NONE");
    while let Some(batch) = receiver.recv().await {
        store
            .query(insert.as_str())
            .bind(("rows", SerdeWrapper(batch)))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .with_context(|| format!("writing to {table}"))?;
    }
    let summary = builder.await.context("the ledger builder panicked")??;

    let build = Build {
        version: ledger::derived_version(),
        table,
        built_at: Utc::now(),
        summary,
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
    tracing::info!(table = %build.table, days = build.summary.days, elapsed_secs = started.elapsed().as_secs(), "ledger rebuilt");
    Ok(build)
}

/// Rebuilds in the background when `ledger:current` was built by another version, so a deploy that
/// changes the parser or the catalog serves the old build until the new one is done. Unknown tags
/// and units are quarantined (Decision 21).
///
/// # Errors
///
/// When `ledger:current` can't be read. A failed rebuild is logged.
pub async fn ensure_current(store: &Store, data_dir: &Path) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    let version = ledger::derived_version();
    if current(store).await?.is_some_and(|build| build.version == version) {
        tracing::info!(%version, "the ledger is current");
        return Ok(None);
    }
    let (store, data_dir) = (store.clone(), data_dir.to_owned());
    Ok(Some(tokio::spawn(async move {
        if let Err(error) = rebuild(&store, &data_dir, Unknowns::Quarantine).await {
            tracing::error!(error = format!("{error:#}"), "the ledger rebuild failed; the previous build keeps serving");
        }
    })))
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

/// Drops every build table except `keep`, a unit at a time and then the empty table.
async fn drop_tables(store: &Store, units: &Units, keep: Option<&str>) -> anyhow::Result<()> {
    for table in tables(store).await? {
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
            .query(format!("REMOVE TABLE {table}"))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .with_context(|| format!("removing {table}"))?;
        tracing::info!(%table, "dropped an old ledger build");
    }
    Ok(())
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
