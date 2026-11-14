//! The update loop (DESIGN.md Decisions 7 and 12). On start, and every six hours after, it fetches
//! what the NRC and EIA have published since, rebuilds the ledger when the raw store's content has
//! changed, and records the run for `/status`. One task does all of it, so two rebuilds never
//! overlap. With fetching off, it runs once, to rebuild at start.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, Days, NaiveDate, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::types::SerdeWrapper;

use downpower_ingest::eia::{self, NucStatus};
use downpower_ingest::fetch::{self, Config, Fetcher, Summary};
use downpower_ingest::nrc::Report;
use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::units::Units;

use crate::db::Store;
use crate::ledger;

/// How often the loop runs. The NRC publishes once a day, so four looks catch a page within hours.
pub const EVERY: Duration = Duration::from_hours(6);

/// The env var that turns fetching on: `daily` on the Fly machine. Unset, `serve` only rebuilds at
/// start, so a dev server never touches nrc.gov.
pub const FETCH_VAR: &str = "DOWNPOWER_FETCH";

/// How old the newest `NUC_STATUS` may get before a run fetches another. A copy is about 6 MB, and
/// EIA's numbers repeat the NRC's (PRD-0002.log.md, T-007).
const NUC_STATUS_AGE: TimeDelta = TimeDelta::days(7);

/// How far behind the newest page past its hold a run looks for pages it hasn't settled, which
/// catches up after downtime.
const CATCH_UP: Days = Days::new(60);

/// The Event Notification pages a run looks back over; each settles two days after its date.
const EVENTS_BACK: Days = Days::new(7);

/// Where a run fetches from.
#[derive(Debug, Clone)]
pub struct Sources {
    pub nrc: Config,
    pub nuc_status_url: String,
}

impl Sources {
    /// nrc.gov at the promised rate, and EIA's bulk file.
    #[must_use]
    pub fn live() -> Self {
        Self {
            nrc: Config::nrc(),
            nuc_status_url: eia::NUC_STATUS_URL.to_owned(),
        }
    }

    /// The live sources when [`FETCH_VAR`] is `daily`, given its value.
    #[must_use]
    pub fn from_env(value: Option<&OsStr>) -> Option<Self> {
        (value == Some("daily".as_ref())).then(Self::live)
    }
}

/// One run, as the `run` table records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    /// What the status and Event Notification fetches did; `None` when the run didn't fetch.
    pub status: Option<Summary>,
    pub events: Option<Summary>,
    /// The last day a `NUC_STATUS` fetched this run covers.
    pub nuc_status: Option<NaiveDate>,
    /// The table a rebuild made current.
    pub rebuilt: Option<String>,
    /// What went wrong. A failed fetch still lets the rebuild run over what the store holds.
    pub errors: Vec<String>,
}

/// Defines the `run` table.
///
/// # Errors
///
/// When the definition fails.
pub async fn define(store: &Store) -> anyhow::Result<()> {
    store
        .query("DEFINE TABLE IF NOT EXISTS run SCHEMALESS")
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("defining the run table")?;
    Ok(())
}

/// Fetches into the raw store under `data_dir` from `sources` (when given), then refreshes the
/// ledger. Never fails: what goes wrong lands in [`Run::errors`].
pub async fn run_once(store: &Store, data_dir: &Path, sources: Option<&Sources>, today: NaiveDate) -> Run {
    let mut run = Run {
        started_at: Utc::now(),
        finished_at: Utc::now(),
        status: None,
        events: None,
        nuc_status: None,
        rebuilt: None,
        errors: Vec::new(),
    };
    if let Some(sources) = sources
        && let Err(error) = fetch(data_dir, sources, today, &mut run).await
    {
        run.errors.push(format!("{error:#}"));
    }
    match ledger::refresh(store, data_dir).await {
        Ok(build) => run.rebuilt = build.map(|build| build.table),
        Err(error) => run.errors.push(format!("{error:#}")),
    }
    run.finished_at = Utc::now();
    run
}

/// Decision 12's fetches: the newest status page (numbers only), the page that has just passed its
/// hold (reasons), any page in between the store lacks, and any older one not yet settled; the
/// recent Event Notification pages; and a `NUC_STATUS` once the stored one is a week old.
async fn fetch(data_dir: &Path, sources: &Sources, today: NaiveDate, run: &mut Run) -> anyhow::Result<()> {
    let raw = RawStore::open(data_dir)?;
    let stored = raw.latest(Series::PowerStatus)?;
    let mut fetcher = Fetcher::new(sources.nrc.clone(), raw.clone())?;
    let past_hold = today - Report::PowerStatus.hold();
    let mut status = fetcher.run(Report::PowerStatus, past_hold - CATCH_UP, past_hold).await?;
    for date in (past_hold + Days::new(1)).iter_days().take_while(|date| *date < today - Days::new(1)) {
        if !stored.contains_key(&date) {
            add(&mut status, fetcher.run(Report::PowerStatus, date, date).await?);
        }
    }
    add(&mut status, fetcher.run(Report::PowerStatus, today - Days::new(1), today).await?);
    run.status = Some(status);
    run.events = Some(fetcher.run(Report::EventNotifications, today - EVENTS_BACK, today).await?);
    let newest_nuc_status = raw.latest(Series::NucStatus)?.into_values().map(|record| record.fetched_at).max();
    if newest_nuc_status.is_none_or(|fetched_at| Utc::now() - fetched_at >= NUC_STATUS_AGE) {
        run.nuc_status = fetch_nuc_status(&raw, &sources.nuc_status_url).await?;
    }
    Ok(())
}

fn add(total: &mut Summary, more: Summary) {
    total.stored += more.stored;
    total.missing += more.missing;
    total.skipped += more.skipped;
}

/// Fetches `NUC_STATUS` from `url` into `raw`, then parses the stored copy and checks every
/// generator against the units catalog, so a format change or a new generator fails here, with
/// what EIA served already on record. Returns the last day it covers.
///
/// # Errors
///
/// When the fetch fails (see [`eia::fetch`]), the file doesn't parse, or a generator is unknown.
pub async fn fetch_nuc_status(raw: &RawStore, url: &str) -> anyhow::Result<Option<NaiveDate>> {
    let record = eia::fetch(&fetch::client()?, url, raw).await?;
    let hash = record.sha256.context("a stored NUC_STATUS has a hash")?;
    let nuc_status = NucStatus::parse(&raw.get(&hash)?).context("parsing the NUC_STATUS just stored")?;
    nuc_status.check(&Units::catalog()?)?;
    tracing::info!(end = ?nuc_status.end(), "NUC_STATUS checks out");
    Ok(nuc_status.end())
}

/// Stores `run` in the `run` table.
///
/// # Errors
///
/// When the write fails.
pub async fn record(store: &Store, run: &Run) -> anyhow::Result<()> {
    store
        .query("CREATE run CONTENT $run RETURN NONE")
        .bind(("run", SerdeWrapper(run.clone())))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .context("recording the run")?;
    Ok(())
}

/// The newest `limit` runs, newest first.
///
/// # Errors
///
/// When the query fails or a record doesn't parse.
pub async fn runs(store: &Store, limit: u32) -> anyhow::Result<Vec<Run>> {
    let runs: Vec<SerdeWrapper<Run>> = store
        .query("SELECT * OMIT id FROM run ORDER BY started_at DESC LIMIT $limit")
        .bind(("limit", limit))
        .await
        .context("reading runs")?
        .take(0)
        .context("reading runs")?;
    Ok(runs.into_iter().map(|run| run.0).collect())
}

/// Starts the loop: a run now and, when `sources` is given, one every [`EVERY`] after.
pub fn spawn(store: Store, data_dir: PathBuf, sources: Option<Sources>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let run = run_once(&store, &data_dir, sources.as_ref(), Utc::now().date_naive()).await;
            if run.errors.is_empty() {
                tracing::info!(status = ?run.status, events = ?run.events, nuc_status = ?run.nuc_status, rebuilt = ?run.rebuilt, "update run done");
            } else {
                tracing::error!(errors = ?run.errors, rebuilt = ?run.rebuilt, "update run failed");
            }
            if let Err(error) = record(&store, &run).await {
                tracing::error!(error = format!("{error:#}"), "recording the run failed");
            }
            if sources.is_none() {
                break;
            }
            tokio::time::sleep(EVERY).await;
        }
    })
}
