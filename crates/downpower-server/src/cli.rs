//! The `downpower` command line: `serve` (the default), `fetch`, `backfill` and `rebuild`.

use std::path::Path;
use std::time::Duration;

use chrono::{NaiveDate, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};

use downpower_ingest::eia;
use downpower_ingest::fetch::{self, Config, Fetcher, Summary};
use downpower_ingest::ledger::Unknowns;
use downpower_ingest::nrc::{self, Report};
use downpower_ingest::raw::RawStore;

use crate::{daily, db, ledger};

/// Why US nuclear reactors lose output.
#[derive(Debug, Parser)]
#[command(name = "downpower", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve the site (the default).
    Serve,
    /// Fetch NRC pages (skipping the pages an earlier run settled) or EIA's nuclear outage file
    /// into the raw store.
    Fetch(FetchArgs),
    /// Fetch every series through today, then rebuild the ledger, failing on any unit or tag the
    /// catalog doesn't know. A first run takes about a day.
    Backfill(Pace),
    /// Rebuild the ledger from the raw store, failing on any unit or tag the catalog doesn't know.
    /// The store must be free: stop `serve` first.
    Rebuild,
}

/// How fast to ask nrc.gov.
#[derive(Debug, Args)]
pub struct Pace {
    /// Seconds between request starts. Never under 5, the rate the NRC email promises.
    #[arg(long, default_value_t = fetch::MIN_INTERVAL.as_secs(), value_parser = clap::value_parser!(u64).range(fetch::MIN_INTERVAL.as_secs()..))]
    pub interval_secs: u64,
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    /// Which series to fetch.
    #[arg(long, value_enum)]
    pub series: SeriesArg,
    /// The first report date.
    #[arg(long, default_value_t = nrc::FIRST_DAY)]
    pub from: NaiveDate,
    /// The last report date [default: today, UTC].
    #[arg(long)]
    pub to: Option<NaiveDate>,
    #[command(flatten)]
    pub pace: Pace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SeriesArg {
    /// Power Reactor Status Reports, daily.
    Status,
    /// Event Notification Reports, business days.
    Events,
    /// EIA's daily nuclear outage bulk file, in one request; the dates and interval don't apply.
    NucStatus,
}

impl SeriesArg {
    /// The NRC report fetched page by page, or `None` for EIA's bulk file.
    #[must_use]
    pub const fn report(self) -> Option<Report> {
        match self {
            Self::Status => Some(Report::PowerStatus),
            Self::Events => Some(Report::EventNotifications),
            Self::NucStatus => None,
        }
    }
}

/// Runs `downpower fetch` into the raw store under `data_dir`.
///
/// # Errors
///
/// When the raw store cannot be opened, the fetcher stops (see [`Fetcher::run`]), or the
/// `NUC_STATUS` just stored doesn't parse or names a generator the units catalog lacks.
pub async fn fetch(args: &FetchArgs, data_dir: &Path) -> anyhow::Result<Summary> {
    let store = RawStore::open(data_dir)?;
    let to = args.to.unwrap_or_else(|| Utc::now().date_naive());
    match args.series.report() {
        Some(report) => fetch_report(store, &args.pace, report, args.from, to).await,
        None => fetch_nuc_status(&store).await,
    }
}

async fn fetch_report(store: RawStore, pace: &Pace, report: Report, from: NaiveDate, to: NaiveDate) -> anyhow::Result<Summary> {
    let config = Config {
        interval: Duration::from_secs(pace.interval_secs),
        ..Config::nrc()
    };
    Fetcher::new(config, store)?.run(report, from, to).await
}

async fn fetch_nuc_status(store: &RawStore) -> anyhow::Result<Summary> {
    daily::fetch_nuc_status(store, eia::NUC_STATUS_URL).await?;
    Ok(Summary { stored: 1, ..Summary::default() })
}

/// Runs `downpower backfill`: every status and Event Notification page from 1999 through today,
/// a fresh `NUC_STATUS`, then [`rebuild`].
///
/// # Errors
///
/// When a fetch stops (see [`fetch`]) or the rebuild fails.
pub async fn backfill(pace: &Pace, data_dir: &Path) -> anyhow::Result<ledger::Build> {
    let today = Utc::now().date_naive();
    for report in [Report::PowerStatus, Report::EventNotifications] {
        let summary = fetch_report(RawStore::open(data_dir)?, pace, report, nrc::FIRST_DAY, today).await?;
        tracing::info!(?report, summary.stored, summary.missing, summary.skipped, "fetched");
    }
    fetch_nuc_status(&RawStore::open(data_dir)?).await?;
    rebuild(data_dir).await
}

/// Runs `downpower rebuild`: the ledger from the raw store under `data_dir`, failing on any unit or
/// tag the catalog doesn't know (Decision 21).
///
/// # Errors
///
/// When the store can't be opened (another process holds it) or the rebuild fails.
pub async fn rebuild(data_dir: &Path) -> anyhow::Result<ledger::Build> {
    let store = db::open(data_dir).await?;
    ledger::rebuild(&store, data_dir, Unknowns::Fail).await
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("downpower").chain(args.iter().copied()))
    }

    #[test]
    fn no_subcommand_means_serve() {
        assert!(parse(&[]).unwrap().command.is_none());
        assert!(matches!(parse(&["serve"]).unwrap().command, Some(Command::Serve)));
    }

    #[test]
    fn fetch_defaults_to_the_whole_archive_at_the_promised_rate() {
        let Some(Command::Fetch(args)) = parse(&["fetch", "--series", "events"]).unwrap().command else {
            panic!("not a fetch")
        };

        assert_eq!(args.series, SeriesArg::Events);
        assert_eq!(args.from, nrc::FIRST_DAY);
        assert_eq!(args.to, None);
        assert_eq!(args.pace.interval_secs, 5);
        assert_eq!(args.series.report(), Some(Report::EventNotifications));
    }

    #[test]
    fn fetch_takes_nuc_status_as_one_bulk_file() {
        let Some(Command::Fetch(args)) = parse(&["fetch", "--series", "nuc-status"]).unwrap().command else {
            panic!("not a fetch")
        };

        assert_eq!(args.series.report(), None);
    }

    #[test]
    fn fetch_refuses_to_go_faster_than_the_promised_rate() {
        assert!(parse(&["fetch", "--series", "status", "--interval-secs", "4"]).is_err());
        assert!(parse(&["fetch", "--series", "status", "--interval-secs", "60"]).is_ok());
        assert!(parse(&["backfill", "--interval-secs", "4"]).is_err());
    }

    #[test]
    fn backfill_and_rebuild_are_subcommands() {
        let Some(Command::Backfill(pace)) = parse(&["backfill"]).unwrap().command else {
            panic!("not a backfill")
        };

        assert_eq!(pace.interval_secs, 5);
        assert!(matches!(parse(&["rebuild"]).unwrap().command, Some(Command::Rebuild)));
    }
}
