//! The update loop (DESIGN.md Decisions 7 and 12). On start, and every six hours after, it fetches
//! what the NRC and EIA have published since, brings the ledger up to date when the raw store's
//! content has changed, and records the run for `/status`. One task does all of it, so two rebuilds never
//! overlap. With fetching off, it runs once, to rebuild at start.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, ensure};
use chrono::{DateTime, Days, NaiveDate, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::types::SerdeWrapper;

use downpower_ingest::eia::{self, NucStatus};
use downpower_ingest::fetch::{self, Config, Fetcher, Summary};
use downpower_ingest::nrc::Report;
use downpower_ingest::raw::{RawStore, Record, Series};
use downpower_ingest::scram_dataset;
use downpower_ingest::status::{self, StatusPage};
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

/// How old the newest scram dataset may get before a run fetches it again. The NRC last changed it
/// 2025-05-13, so a weekly look is plenty (DESIGN.md Decision 33).
const SCRAM_DATASET_AGE: TimeDelta = TimeDelta::days(7);

/// How far behind the newest page past its hold a run looks for pages it hasn't settled, which
/// catches up after downtime.
const CATCH_UP: Days = Days::new(60);

/// How far back from today a run fetches every page again, whatever the store holds: yesterday's
/// and today's status pages, and the last week of Event Notification pages (Decision 12).
const fn refetched(report: Report) -> Days {
    match report {
        Report::PowerStatus => Days::new(1),
        Report::EventNotifications => Days::new(7),
    }
}

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
    /// The last day a scram dataset fetched this run covers.
    #[serde(default)]
    pub scram_dataset: Option<NaiveDate>,
    /// The table a rebuild made current or an update wrote into.
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
        scram_dataset: None,
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

/// Decision 12's fetches: the status and Event Notification pages [`fetch_report`] names, and a
/// `NUC_STATUS` and the scram dataset once the stored copy is a week old.
async fn fetch(data_dir: &Path, sources: &Sources, today: NaiveDate, run: &mut Run) -> anyhow::Result<()> {
    let raw = RawStore::open(data_dir)?;
    let mut fetcher = Fetcher::new(sources.nrc.clone(), raw.clone())?;
    run.status = Some(fetch_report(&mut fetcher, &raw, Report::PowerStatus, today).await?);
    run.events = Some(fetch_report(&mut fetcher, &raw, Report::EventNotifications, today).await?);
    let newest_nuc_status = raw.latest(Series::NucStatus)?.into_values().map(|record| record.fetched_at).max();
    if newest_nuc_status.is_none_or(|fetched_at| Utc::now() - fetched_at >= NUC_STATUS_AGE) {
        run.nuc_status = fetch_nuc_status(&raw, &sources.nuc_status_url).await?;
    }
    let newest_dataset = raw.latest(Series::ScramDataset)?.into_values().map(|record| record.fetched_at).max();
    if newest_dataset.is_none_or(|fetched_at| Utc::now() - fetched_at >= SCRAM_DATASET_AGE) {
        run.scram_dataset = Some(fetch_scram_dataset(&mut fetcher, &raw, today).await?);
    }
    Ok(())
}

/// Fetches the scram dataset into `raw` under `today`, then parses the stored copy, so a format
/// change fails here, with what the NRC served already on record. Returns the last day it covers.
///
/// # Errors
///
/// When the fetch fails (see [`Fetcher::file`]), nrc.gov no longer has the file, or it doesn't
/// parse or has no rows.
pub async fn fetch_scram_dataset(fetcher: &mut Fetcher, raw: &RawStore, today: NaiveDate) -> anyhow::Result<NaiveDate> {
    let summary = fetcher.file(Series::ScramDataset, scram_dataset::PATH, today).await?;
    ensure!(summary.missing == 0, "the scram dataset is gone from {}", scram_dataset::PATH);
    let hash = raw
        .latest(Series::ScramDataset)?
        .into_values()
        .next_back()
        .and_then(|record| record.sha256)
        .context("a stored scram dataset has a hash")?;
    let rows = scram_dataset::parse(&raw.get(&hash)?).context("parsing the scram dataset just stored")?;
    let last = rows.iter().map(|row| row.date).max().context("the scram dataset just stored has no rows")?;
    tracing::info!(rows = rows.len(), %last, "the scram dataset checks out");
    Ok(last)
}

/// One report's pages: every unsettled page from [`CATCH_UP`] before the hold's edge through the
/// edge, the page that has just passed its hold among them; each date between the edge and the
/// newest pages whose stored copy has no report; and the newest pages, as [`refetched`] counts them.
async fn fetch_report(fetcher: &mut Fetcher, raw: &RawStore, report: Report, today: NaiveDate) -> anyhow::Result<Summary> {
    let edge = today - report.hold();
    let newest = today - refetched(report);
    let mut summary = fetcher.run(report, edge - CATCH_UP, edge).await?;
    let stored = raw.latest(report.into())?;
    for date in (edge + Days::new(1)).iter_days().take_while(|date| *date < newest) {
        if !reported(raw, report, stored.get(&date))? {
            add(&mut summary, fetcher.run(report, date, date).await?);
        }
    }
    add(&mut summary, fetcher.run(report, newest, today).await?);
    Ok(summary)
}

/// Whether a date's newest capture holds its report. No capture or a 404 doesn't, and neither does
/// a status page reading "No data available", as a day's page does until the NRC publishes it
/// (Decision 29). A status page that doesn't parse counts as reported: fetching it again wouldn't
/// change the parser, and the rebuild reports it.
fn reported(raw: &RawStore, report: Report, newest: Option<&Record>) -> anyhow::Result<bool> {
    let Some(hash) = newest.and_then(|record| record.sha256.as_ref()) else {
        return Ok(false);
    };
    Ok(match report {
        Report::PowerStatus => !matches!(status::parse(&raw.get(hash)?), Ok(StatusPage::NoData)),
        Report::EventNotifications => true,
    })
}

fn add(total: &mut Summary, more: Summary) {
    total.stored += more.stored;
    total.unchanged += more.unchanged;
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
