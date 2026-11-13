//! The `downpower` command line: `serve` (the default) and `fetch`.

use std::path::Path;
use std::time::Duration;

use chrono::{NaiveDate, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};

use downpower_ingest::fetch::{self, Config, Fetcher, Summary};
use downpower_ingest::nrc;
use downpower_ingest::raw::{RawStore, Series};

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
    /// Fetch NRC pages into the raw store, skipping the pages an earlier run settled.
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
}

impl From<SeriesArg> for Series {
    fn from(series: SeriesArg) -> Self {
        match series {
            SeriesArg::Status => Self::PowerStatus,
            SeriesArg::Events => Self::EventNotifications,
        }
    }
}

/// Runs `downpower fetch` against nrc.gov into the raw store under `data_dir`.
///
/// # Errors
///
/// When the raw store cannot be opened, or the fetcher stops (see [`Fetcher::run`]).
pub async fn fetch(args: &FetchArgs, data_dir: &Path) -> anyhow::Result<Summary> {
    let store = RawStore::open(data_dir)?;
    let config = Config {
        interval: Duration::from_secs(args.interval_secs),
        ..Config::nrc()
    };
    let to = args.to.unwrap_or_else(|| Utc::now().date_naive());
    Fetcher::new(config, store)?.run(args.series.into(), args.from, to).await
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
        assert_eq!(Series::from(args.series), Series::EventNotifications);
    }

    #[test]
    fn fetch_refuses_to_go_faster_than_the_promised_rate() {
        assert!(parse(&["fetch", "--series", "status", "--interval-secs", "4"]).is_err());
        assert!(parse(&["fetch", "--series", "status", "--interval-secs", "60"]).is_ok());
    }
}
