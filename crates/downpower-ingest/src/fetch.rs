//! The polite fetcher (DESIGN.md Decision 10). It sends one request per interval under a User-Agent
//! that names the project, and backs off on 429s, 5xx responses, network errors and truncated
//! bodies. A 403 stops the run, since it would mean the NRC's bot block is back, and that calls for
//! a person rather than a retry. A rerun resumes by skipping the pages an earlier run settled.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Context as _, bail};
use chrono::{NaiveDate, Utc};
use reqwest::header::RETRY_AFTER;
use reqwest::{Response, StatusCode};
use tokio::time::{Instant, sleep, sleep_until};

use crate::Res;
use crate::nrc::{self, Report};
use crate::raw::{RawStore, Record, Series, Source};

/// The User-Agent the NRC email names: `downpower/0.1 (+https://github.com/twitchax/downpower)`.
pub const USER_AGENT: &str = concat!(
    "downpower/",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    " (+https://github.com/twitchax/downpower)"
);

/// An HTTP client that sends [`USER_AGENT`], for every source.
///
/// # Errors
///
/// When the client cannot be built (e.g., no TLS roots).
pub fn client() -> Res<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .build()
        .context("building the HTTP client")
}

/// The fastest the fetcher may go against nrc.gov: the rate the NRC email promises.
pub const MIN_INTERVAL: Duration = Duration::from_secs(5);

/// How the fetcher paces and retries.
#[derive(Debug, Clone)]
pub struct Config {
    /// The site root, with no trailing slash.
    pub base_url: String,
    /// The least time between the starts of two requests.
    pub interval: Duration,
    /// The wait before the first retry. Each later retry doubles it, up to `max_backoff`.
    pub backoff: Duration,
    pub max_backoff: Duration,
    /// Tries per page before the run fails.
    pub attempts: u32,
}

impl Config {
    /// nrc.gov at [`MIN_INTERVAL`], with a minute's backoff doubling to half an hour, and eight tries
    /// per page (about two hours of retrying before the run gives up).
    #[must_use]
    pub fn nrc() -> Self {
        Self {
            base_url: nrc::BASE_URL.to_owned(),
            interval: MIN_INTERVAL,
            backoff: Duration::from_mins(1),
            max_backoff: Duration::from_mins(30),
            attempts: 8,
        }
    }
}

/// What a run did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Pages stored.
    pub stored: usize,
    /// Dates with no page (a 404).
    pub missing: usize,
    /// Dates an earlier run settled.
    pub skipped: usize,
}

/// How one page ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Stored,
    Missing,
}

/// How one request ended: the page's outcome, or a retry after at least the given wait.
enum Attempt {
    Done(Outcome),
    Retry(Option<Duration>),
}

/// Spaces the starts of requests at least `interval` apart.
#[derive(Debug)]
struct Pacer {
    interval: Duration,
    last: Option<Instant>,
}

impl Pacer {
    async fn wait(&mut self) {
        if let Some(last) = self.last {
            sleep_until(last + self.interval).await;
        }
        self.last = Some(Instant::now());
    }
}

/// Fetches pages into a [`RawStore`].
#[derive(Debug)]
pub struct Fetcher {
    client: reqwest::Client,
    config: Config,
    store: RawStore,
    pacer: Pacer,
}

impl Fetcher {
    /// A fetcher writing into `store`.
    ///
    /// # Errors
    ///
    /// When the HTTP client cannot be built (e.g., no TLS roots).
    pub fn new(config: Config, store: RawStore) -> Res<Self> {
        let client = client()?;
        let pacer = Pacer { interval: config.interval, last: None };
        Ok(Self { client, config, store, pacer })
    }

    /// Fetches every unsettled page of `series` dated `from..=to`, oldest first.
    ///
    /// # Errors
    ///
    /// On a 403, on any status other than 200, 404, 429 and 5xx, when a page fails every attempt, and
    /// when the raw store cannot be read or written.
    pub async fn run(&mut self, report: Report, from: NaiveDate, to: NaiveDate) -> Res<Summary> {
        let series = Series::from(report);
        let settled: HashSet<NaiveDate> = self
            .store
            .records()?
            .into_iter()
            .filter(|record| record.series == series && nrc::settles(record))
            .map(|record| record.date)
            .collect();
        let mut summary = Summary::default();
        for date in report.dates(from, to) {
            if settled.contains(&date) {
                summary.skipped += 1;
                continue;
            }
            match self.fetch(report, date).await? {
                Outcome::Stored => summary.stored += 1,
                Outcome::Missing => summary.missing += 1,
            }
            if (summary.stored + summary.missing) % 100 == 0 {
                tracing::info!(?report, %date, summary.stored, summary.missing, summary.skipped, "progress");
            }
        }
        Ok(summary)
    }

    async fn fetch(&mut self, report: Report, date: NaiveDate) -> Res<Outcome> {
        let url = format!("{}{}", self.config.base_url, report.path(date));
        let mut backoff = self.config.backoff;
        for attempt in 1..=self.config.attempts {
            self.pacer.wait().await;
            match self.attempt(report, date, &url).await? {
                Attempt::Done(outcome) => return Ok(outcome),
                Attempt::Retry(retry_after) if attempt < self.config.attempts => {
                    let wait = retry_after.map_or(backoff, |asked| asked.max(backoff));
                    tracing::warn!(%url, attempt, wait_secs = wait.as_secs_f64(), "retrying");
                    sleep(wait).await;
                    backoff = backoff.saturating_mul(2).min(self.config.max_backoff);
                }
                Attempt::Retry(_) => {}
            }
        }
        bail!("{url} failed {} times in a row, so the run stops here", self.config.attempts)
    }

    async fn attempt(&self, report: Report, date: NaiveDate, url: &str) -> Res<Attempt> {
        let fetched_at = Utc::now();
        let response = match self.client.get(url).send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%url, %error, "request failed");
                return Ok(Attempt::Retry(None));
            }
        };
        let status = response.status();
        let record = |sha256, len| Record {
            source: Source::Nrc,
            series: report.into(),
            date,
            url: url.to_owned(),
            status: status.as_u16(),
            captured_at: fetched_at,
            fetched_at,
            sha256,
            len,
        };
        match status {
            StatusCode::OK => {
                let body = match response.bytes().await {
                    Ok(body) if looks_complete(&body) => body,
                    Ok(body) => {
                        tracing::warn!(%url, len = body.len(), "truncated body");
                        return Ok(Attempt::Retry(None));
                    }
                    Err(error) => {
                        tracing::warn!(%url, %error, "reading the body failed");
                        return Ok(Attempt::Retry(None));
                    }
                };
                let hash = self.store.put(&body)?;
                self.store.append(&record(Some(hash), u64::try_from(body.len()).ok()))?;
                tracing::info!(?report, %date, len = body.len(), "stored");
                Ok(Attempt::Done(Outcome::Stored))
            }
            StatusCode::NOT_FOUND => {
                self.store.append(&record(None, None))?;
                tracing::info!(?report, %date, "no page");
                Ok(Attempt::Done(Outcome::Missing))
            }
            StatusCode::FORBIDDEN => bail!("{url} returned 403. The NRC's bot block may be back, so the run stops here (docs/research/data-sources.md, \"The access problem\")"),
            StatusCode::TOO_MANY_REQUESTS => Ok(Attempt::Retry(retry_after(&response))),
            status if status.is_server_error() => Ok(Attempt::Retry(retry_after(&response))),
            status => bail!("{url} returned {status}"),
        }
    }
}

/// The server's `Retry-After`, when it is given in seconds.
fn retry_after(response: &Response) -> Option<Duration> {
    let seconds = response.headers().get(RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

/// Whether a 200 body ends its document. A connection cut mid-page still arrives as a 200.
fn looks_complete(body: &[u8]) -> bool {
    body[body.len().saturating_sub(4096)..].windows(7).any(|window| window.eq_ignore_ascii_case(b"</html>"))
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_user_agent_is_the_one_the_nrc_email_names() {
        assert_eq!(USER_AGENT, "downpower/0.1 (+https://github.com/twitchax/downpower)");
    }

    #[test]
    fn a_body_is_complete_only_when_it_closes_its_document() {
        assert!(looks_complete(b"<html><body></body></HTML>\n"));
        assert!(!looks_complete(b"<html><body><table><tr><td>Limerick 1"));
        assert!(!looks_complete(b""));
    }

    #[tokio::test(start_paused = true)]
    async fn the_pacer_spaces_request_starts_by_the_interval() {
        let mut pacer = Pacer {
            interval: Duration::from_secs(5),
            last: None,
        };
        let start = Instant::now();

        pacer.wait().await;
        let first = Instant::now();
        pacer.wait().await;
        let second = Instant::now();
        tokio::time::advance(Duration::from_secs(7)).await;
        pacer.wait().await;
        let third = Instant::now();

        assert_eq!(first - start, Duration::ZERO);
        assert_eq!(second - first, Duration::from_secs(5));
        // A request that comes late after a long gap goes right away.
        assert_eq!(third - second, Duration::from_secs(7));
    }
}
