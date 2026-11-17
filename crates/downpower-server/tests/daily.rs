//! The update loop (PRD-0002 T-011, PRD-0003 T-006) against a fake nrc.gov and EIA on loopback: a
//! run fetches what Decision 12 names, rebuilds, and records itself; a second run the same day
//! refetches only the pages still inside their hold that need it and doesn't rebuild.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use chrono::{Days, NaiveDate, Utc};
use pretty_assertions::assert_eq;

use downpower_ingest::fetch::Config;
use downpower_ingest::raw::{RawStore, Record, Series, Source};
use downpower_server::daily::{self, Sources};
use downpower_server::tests::{block_on, fixture, raw_store};
use downpower_server::{db, ledger};

/// The fake site's pages: the 2025-07-15 status page for every status path, a stub for every Event
/// Notification path, and the `NUC_STATUS` fixture zipped.
fn site() -> impl Fn(&str) -> Response + Clone + Send + Sync + 'static {
    let page = fixture("status/2025-07-15.html");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("NUC_STATUS.txt", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(&fixture("eia/nuc_status.txt")).unwrap();
    let zip = zip.finish().unwrap().into_inner();
    move |path: &str| {
        if path.contains("power-reactor-status-reports") {
            page.clone().into_response()
        } else if path.contains("event-notification-reports") {
            "<html><body>No events.</body></html>".into_response()
        } else if path.ends_with("NUC_STATUS.zip") {
            zip.clone().into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// Serves `respond` on loopback. Returns the base URL and every path requested.
async fn serve(respond: impl Fn(&str) -> Response + Clone + Send + Sync + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
    let paths: Arc<Mutex<Vec<String>>> = Arc::default();
    let app = Router::new().fallback({
        let paths = Arc::clone(&paths);
        move |uri: Uri| {
            let path = uri.path().to_owned();
            paths.lock().unwrap().push(path.clone());
            let response = respond(&path);
            async move { response }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), paths)
}

/// The fake site, or `status` for every path when it isn't 200.
async fn fake(status: StatusCode) -> (String, Arc<Mutex<Vec<String>>>) {
    let site = site();
    serve(move |path| if status == StatusCode::OK { site(path) } else { status.into_response() }).await
}

fn sources(base: &str) -> Sources {
    Sources {
        nrc: Config {
            base_url: base.to_owned(),
            interval: Duration::from_millis(1),
            backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
            attempts: 1,
        },
        nuc_status_url: format!("{base}/opendata/bulk/NUC_STATUS.zip"),
    }
}

fn status_paths(paths: &Mutex<Vec<String>>) -> Vec<String> {
    paths.lock().unwrap().iter().filter(|path| path.contains("power-reactor-status-reports")).cloned().collect()
}

/// The dates of the Event Notification pages requested, in order.
fn event_dates(paths: &Mutex<Vec<String>>) -> Vec<NaiveDate> {
    let paths = paths.lock().unwrap();
    let pages = paths.iter().filter_map(|path| path.rsplit('/').next()?.strip_suffix("en"));
    pages.map(|page| NaiveDate::parse_from_str(page, "%Y%m%d").unwrap()).collect()
}

/// Every date from `from` through `to`.
fn days(from: NaiveDate, to: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    from.iter_days().take_while(move |date| *date <= to)
}

#[test]
fn daily_a_run_fetches_rebuilds_and_records_itself() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let store = db::open(tmp.path()).await.unwrap();
        let (base, paths) = fake(StatusCode::OK).await;
        let today = Utc::now().date_naive();

        let first = daily::run_once(&store, tmp.path(), Some(&sources(&base)), today).await;
        daily::record(&store, &first).await.unwrap();
        let fetched_first = status_paths(&paths);
        paths.lock().unwrap().clear();
        let second = daily::run_once(&store, tmp.path(), Some(&sources(&base)), today).await;

        assert_eq!(first.errors, Vec::<String>::new());
        // 61 pages past the hold, the 26 between it and yesterday, then yesterday and today.
        assert_eq!(first.status.unwrap().stored, 61 + 26 + 2);
        assert_eq!(fetched_first.len(), 89);
        // The same for Event Notification pages, but for the newest week in place of two days.
        assert_eq!(first.events.unwrap().stored, 61 + 20 + 8);
        assert!(first.nuc_status.is_some());
        let current = ledger::current(&store).await.unwrap().unwrap();
        assert_eq!(first.rebuilt, Some(current.table));
        assert!(current.summary.days > 0);
        assert_eq!(daily::runs(&store, 10).await.unwrap(), [first]);

        // The pages inside the hold come again, identical, so nothing rebuilds.
        let held: Vec<_> = status_paths(&paths);
        assert_eq!(second.errors, Vec::<String>::new());
        assert_eq!(held.len(), 2, "{held:?}");
        assert!(held[1].ends_with(&format!("{}ps", today.format("%Y%m%d"))));
        assert_eq!((second.nuc_status, second.rebuilt), (None, None));
        let raw = RawStore::open(tmp.path()).unwrap();
        assert_eq!(raw.latest(Series::NucStatus).unwrap().len(), 1);
        assert!(raw.latest(Series::PowerStatus).unwrap().contains_key(&(today - Days::new(28))));
    });
}

#[test]
fn daily_a_failed_fetch_still_rebuilds_from_the_store() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let (base, _) = fake(StatusCode::FORBIDDEN).await;

        let run = daily::run_once(&store, tmp.path(), Some(&sources(&base)), Utc::now().date_naive()).await;

        assert_eq!(run.errors.len(), 1);
        assert!(run.errors[0].contains("403"), "{:?}", run.errors);
        assert!(run.rebuilt.is_some());
    });
}

#[test]
fn daily_without_sources_runs_once_to_rebuild() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();

        daily::spawn(store.clone(), tmp.path().to_owned(), None).await.unwrap();

        let runs = daily::runs(&store, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].status, runs[0].events), (None, None));
        assert_eq!(runs[0].rebuilt, ledger::current(&store).await.unwrap().map(|build| build.table));
    });
}

#[test]
fn daily_fetching_is_on_only_when_the_env_says_daily() {
    assert!(Sources::from_env(None).is_none());
    assert!(Sources::from_env(Some("yes".as_ref())).is_none());
    assert!(Sources::from_env(Some("daily".as_ref())).is_some());
}

#[test]
fn uat_reasons_snapshot_an_unknown_tag_is_quarantined_on_the_daily_path() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let page = String::from_utf8(fixture("status/2025-07-15.html"))
            .unwrap()
            .replace("Conservative Operations", "Zorbulating the Flux Capacitor");
        raw_store(tmp.path(), Some(page.as_bytes()));
        let store = db::open(tmp.path()).await.unwrap();

        let run = daily::run_once(&store, tmp.path(), None, Utc::now().date_naive()).await;

        assert_eq!(run.errors, Vec::<String>::new());
        let current = ledger::current(&store).await.unwrap().unwrap();
        assert_eq!(run.rebuilt, Some(current.table));
        let uses = page.matches("Zorbulating").count() as u64;
        assert_eq!(current.summary.unknown_tags.into_iter().collect::<Vec<_>>(), [("zorbulating the flux capacitor".to_owned(), uses)]);
    });
}

/// Records an Event Notification page for `date` as captured on `at`.
fn capture(raw: &RawStore, date: NaiveDate, at: NaiveDate) {
    let at = at.and_hms_opt(12, 0, 0).unwrap().and_utc();
    let page = b"<html><body>No events.</body></html>";
    raw.append(&Record {
        source: Source::Nrc,
        series: Series::EventNotifications,
        date,
        url: String::new(),
        status: 200,
        captured_at: at,
        fetched_at: at,
        sha256: Some(raw.put(page).unwrap()),
        len: Some(page.len() as u64),
    })
    .unwrap();
}

#[test]
fn uat_fetch_hold_an_event_page_settles_28_days_after_its_date() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let today = Utc::now().date_naive();
        let edge = today - Days::new(28);
        {
            let raw = RawStore::open(tmp.path()).unwrap();
            // Captured two days after its date, which settled it under the old 2-day hold.
            capture(&raw, edge, edge + Days::new(2));
            // Captured 28 days after its date.
            capture(&raw, edge - Days::new(1), edge + Days::new(27));
            // Inside the hold, and already stored.
            capture(&raw, today - Days::new(20), today - Days::new(18));
        }
        let store = db::open(tmp.path()).await.unwrap();
        let (base, paths) = fake(StatusCode::OK).await;

        let first = daily::run_once(&store, tmp.path(), Some(&sources(&base)), today).await;
        let fetched_first = event_dates(&paths);
        paths.lock().unwrap().clear();
        let second = daily::run_once(&store, tmp.path(), Some(&sources(&base)), today).await;

        assert_eq!((first.errors, second.errors), (Vec::<String>::new(), Vec::<String>::new()));
        // 60 days before the hold's edge through today, but for the page settled 28 days after its
        // date and the one stored inside the hold.
        let unsettled: Vec<_> = days(edge - Days::new(60), today).filter(|date| ![edge - Days::new(1), today - Days::new(20)].contains(date)).collect();
        assert_eq!(fetched_first, unsettled);
        // The edge's page settled when it was fetched again, so only the newest week comes again.
        assert_eq!(event_dates(&paths), days(today - Days::new(7), today).collect::<Vec<_>>());
    });
}

#[test]
fn uat_fetch_hold_a_status_date_whose_page_has_no_report_is_refetched_until_one_appears() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let store = db::open(tmp.path()).await.unwrap();
        let today = Utc::now().date_naive();
        let late = format!("{}ps", (today - Days::new(10)).format("%Y%m%d"));
        let published = Arc::new(AtomicBool::new(false));
        let (base, paths) = serve({
            let (site, late, published, no_data) = (site(), late.clone(), Arc::clone(&published), fixture("status/1999-05-08.html"));
            move |path| {
                if path.ends_with(&late) && !published.load(Ordering::SeqCst) {
                    no_data.clone().into_response()
                } else {
                    site(path)
                }
            }
        })
        .await;
        let run = async || {
            paths.lock().unwrap().clear();
            let run = daily::run_once(&store, tmp.path(), Some(&sources(&base)), today).await;
            assert_eq!(run.errors, Vec::<String>::new());
            status_paths(&paths).iter().map(|path| path.rsplit('/').next().unwrap().to_owned()).collect::<Vec<_>>()
        };
        let newest = [today - Days::new(1), today].map(|date| format!("{}ps", date.format("%Y%m%d")));

        let first = run().await;
        let no_report = run().await;
        published.store(true, Ordering::SeqCst);
        let report = run().await;
        let after = run().await;

        assert!(first.contains(&late));
        assert_eq!(no_report, [late.clone(), newest[0].clone(), newest[1].clone()]);
        assert_eq!(report, [late.clone(), newest[0].clone(), newest[1].clone()]);
        assert_eq!(after, newest);
    });
}
