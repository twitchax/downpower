//! The fetcher against a fake nrc.gov on loopback: pacing, retries, 404s, the 403 stop, and resume.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderMap, StatusCode, Uri, header::USER_AGENT};
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use tokio::time::Instant;

use downpower_ingest::fetch::{self, Config, Fetcher, Summary};
use downpower_ingest::nrc::Report;
use downpower_ingest::raw::RawStore;

/// Every request the fake saw: when, which path, and the User-Agent.
type Log = Arc<Mutex<Vec<(Instant, String, String)>>>;

fn page(date: &str) -> String {
    format!("<!DOCTYPE html><html><body><h1>{date}</h1><table></table></body></html>")
}

/// Serves `respond(path, nth request for that path)` on a loopback port, returning its base URL.
async fn fake(respond: fn(&str, usize) -> Response) -> (String, Log) {
    let log: Log = Arc::default();
    let counts: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let app = Router::new().fallback({
        let log = Arc::clone(&log);
        move |uri: Uri, headers: HeaderMap| {
            let path = uri.path().to_owned();
            let agent = headers.get(USER_AGENT).map(|value| value.to_str().unwrap().to_owned()).unwrap_or_default();
            log.lock().unwrap().push((Instant::now(), path.clone(), agent));
            let nth = {
                let mut counts = counts.lock().unwrap();
                let count = counts.entry(path.clone()).or_default();
                *count += 1;
                *count
            };
            async move { respond(&path, nth) }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), log)
}

fn config(base_url: String) -> Config {
    Config {
        base_url,
        interval: Duration::from_millis(100),
        backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(50),
        attempts: 3,
    }
}

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

/// 01-01 is fine, 01-02 is rate limited once, 01-03 has no page, and 01-04 is cut off once.
fn scripted(path: &str, nth: usize) -> Response {
    let date = &path[path.len() - 10..path.len() - 2];
    match (date, nth) {
        ("19990102", 1) => (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "0")]).into_response(),
        ("19990103", _) => StatusCode::NOT_FOUND.into_response(),
        ("19990104", 1) => "<!DOCTYPE html><html><body><table><tr><td>Limer".into_response(),
        _ => page(date).into_response(),
    }
}

#[tokio::test]
async fn uat_fetch_polite_paces_retries_and_resumes() {
    let (base, log) = fake(scripted).await;
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();

    let summary = Fetcher::new(config(base.clone()), store.clone())
        .unwrap()
        .run(Report::PowerStatus, day("1999-01-01"), day("1999-01-04"))
        .await
        .unwrap();

    assert_eq!(summary, Summary { stored: 3, missing: 1, skipped: 0 });
    let hits = log.lock().unwrap().clone();
    let paths: Vec<_> = hits.iter().map(|(_, path, _)| &path[path.len() - 10..]).collect();
    assert_eq!(paths, ["19990101ps", "19990102ps", "19990102ps", "19990103ps", "19990104ps", "19990104ps"]);
    assert!(hits.iter().all(|(_, _, agent)| agent == fetch::USER_AGENT));
    for pair in hits.windows(2) {
        // The pacer spaces request starts 100 ms apart; the slack covers loopback scheduling only.
        assert!(pair[1].0 - pair[0].0 >= Duration::from_millis(90), "requests {:?} apart", pair[1].0 - pair[0].0);
    }

    let records = store.records().unwrap();
    let statuses: Vec<_> = records.iter().map(|record| (record.date.to_string(), record.status)).collect();
    assert_eq!(
        statuses,
        [
            ("1999-01-01".to_owned(), 200),
            ("1999-01-02".to_owned(), 200),
            ("1999-01-03".to_owned(), 404),
            ("1999-01-04".to_owned(), 200)
        ]
    );
    let stored = store.get(records[3].sha256.as_ref().unwrap()).unwrap();
    assert_eq!(String::from_utf8(stored).unwrap(), page("19990104"));
    assert_eq!(records[2].sha256, None);

    let rerun = Fetcher::new(config(base), store.clone())
        .unwrap()
        .run(Report::PowerStatus, day("1999-01-01"), day("1999-01-04"))
        .await
        .unwrap();

    assert_eq!(rerun, Summary { stored: 0, missing: 0, skipped: 4 });
    assert_eq!(log.lock().unwrap().len(), hits.len());
}

#[tokio::test]
async fn uat_fetch_polite_stops_on_403() {
    let (base, log) = fake(|_, _| StatusCode::FORBIDDEN.into_response()).await;
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();

    let error = Fetcher::new(config(base), store.clone())
        .unwrap()
        .run(Report::PowerStatus, day("1999-01-01"), day("1999-01-04"))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("403"), "{error}");
    assert_eq!(log.lock().unwrap().len(), 1);
    assert_eq!(store.records().unwrap(), vec![]);
}

#[tokio::test]
async fn uat_fetch_polite_gives_up_after_its_attempts() {
    let (base, log) = fake(|_, _| StatusCode::SERVICE_UNAVAILABLE.into_response()).await;
    let tmp = tempfile::tempdir().unwrap();

    let error = Fetcher::new(config(base), RawStore::open(tmp.path()).unwrap())
        .unwrap()
        .run(Report::PowerStatus, day("1999-01-01"), day("1999-01-01"))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("failed 3 times"), "{error}");
    assert_eq!(log.lock().unwrap().len(), 3);
}
