//! The `downpower` command line: `serve` (the default) and `fetch`.

use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use chrono::{NaiveDate, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};

use downpower_ingest::eia::{self, NucStatus};
use downpower_ingest::fetch::{self, Config, Fetcher, Summary};
use downpower_ingest::nrc::{self, Report};
use downpower_ingest::raw::RawStore;
use downpower_ingest::units::Units;

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
    /// Seconds between request starts. Never under 5, the rate the NRC email promises.
    #[arg(long, default_value_t = fetch::MIN_INTERVAL.as_secs(), value_parser = clap::value_parser!(u64).range(fetch::MIN_INTERVAL.as_secs()..))]
    pub interval_secs: u64,
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
    let Some(report) = args.series.report() else {
        let record = eia::fetch(&fetch::client()?, eia::NUC_STATUS_URL, &store).await?;
        // What EIA served is stored either way; a format change or a new generator fails here.
        let hash = record.sha256.context("a stored NUC_STATUS has a hash")?;
        let nuc_status = NucStatus::parse(&store.get(&hash)?).context("parsing the NUC_STATUS just stored")?;
        nuc_status.check(&Units::catalog()?)?;
        tracing::info!(end = ?nuc_status.end(), "NUC_STATUS checks out");
        return Ok(Summary { stored: 1, ..Summary::default() });
    };
    let config = Config {
        interval: Duration::from_secs(args.interval_secs),
        ..Config::nrc()
    };
    let to = args.to.unwrap_or_else(|| Utc::now().date_naive());
    Fetcher::new(config, store)?.run(report, args.from, to).await
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
        assert_eq!(args.interval_secs, 5);
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
    }
}
