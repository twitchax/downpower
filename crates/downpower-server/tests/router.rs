//! The real router, in-process: no socket, no built site. The `/api` tests rebuild the ledger over
//! the fixture raw store first (the 2025-07-15 status page and the `NUC_STATUS` fixture).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    Router,
    body::Body,
    http::{
        Request, StatusCode,
        header::{CACHE_CONTROL, CONTENT_ENCODING, ETAG, IF_NONE_MATCH},
    },
    response::Response,
};
use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex};
use tower::ServiceExt as _;
use tracing::instrument::WithSubscriber as _;

use downpower_core::Family;
use downpower_core::barcode::{Barcode, Cell, Layer};
use downpower_ingest::ledger::{UnitDay, Unknowns};
use downpower_ingest::units::Units;
use downpower_server::tests::{block_on, options, raw_store};
use downpower_server::{db, ledger, router};

async fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> Response {
    let mut request = Request::get(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    router.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap()
}

async fn body(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec()
}

#[test]
fn health_is_ok_and_revalidates() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let router = router(options(), db::open(tmp.path()).await.unwrap());
        let response = get(&router, "/healthz", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
    });
}

#[test]
fn api_answers_503_until_a_build_is_current() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let router = router(options(), db::open(tmp.path()).await.unwrap());
        assert_eq!(get(&router, "/api/barcode/attributed", &[]).await.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(get(&router, "/api/unit-day/05000323/2025-07-15", &[]).await.status(), StatusCode::SERVICE_UNAVAILABLE);
    });
}

#[test]
fn uat_barcode_buffer_is_served_per_layer_with_an_etag_stable_while_the_data_is() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let router = router(options(), store.clone());

        let response = get(&router, "/api/barcode/attributed", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        let etag = response.headers()[ETAG].to_str().unwrap().to_owned();
        assert!(etag.starts_with("W/\""), "{etag}");
        let barcode = Barcode::decode(&body(response).await).unwrap();

        // A row per catalog unit, in docket order, and Diablo Canyon 2's reported day.
        let units = Units::catalog().unwrap();
        assert_eq!(barcode.units().len(), units.all().len());
        assert!(barcode.units().windows(2).all(|pair| pair[0].docket < pair[1].docket));
        let row = barcode.units().iter().position(|unit| unit.docket == "05000323").unwrap();
        let index = barcode.index("2025-07-15".parse().unwrap()).unwrap();
        let cell = barcode.cell(row, index).unwrap();
        let day: UnitDay = serde_json::from_slice(&body(get(&router, "/api/unit-day/05000323/2025-07-15", &[]).await).await).unwrap();
        assert_eq!(cell, day.cell(Layer::Attributed));

        // Revalidation answers 304 while nothing changed, and a rebuild of the same raw store
        // keeps the tag.
        let revalidated = get(&router, "/api/barcode/attributed", &[("if-none-match", &etag)]).await;
        assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let rebuilt = get(&router, "/api/barcode/attributed", &[(IF_NONE_MATCH.as_str(), &etag)]).await;
        assert_eq!(rebuilt.status(), StatusCode::NOT_MODIFIED);

        // The observed layer is its own buffer (identical here, since the fixture's one page names
        // every cause it has), and gzip applies when asked for.
        let observed = get(&router, "/api/barcode/observed", &[("accept-encoding", "gzip")]).await;
        assert_eq!(observed.status(), StatusCode::OK);
        assert_eq!(observed.headers()[CONTENT_ENCODING], "gzip");
        assert_eq!(get(&router, "/api/barcode/bogus", &[]).await.status(), StatusCode::NOT_FOUND);
    });
}

#[test]
fn uat_barcode_buffer_moves_its_etag_when_the_data_moves() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let router = router(options(), store.clone());
        let before = get(&router, "/api/barcode/attributed", &[]).await.headers()[ETAG].clone();

        // Another status page for 07-16 changes the data.
        let page = downpower_server::tests::fixture("status/2025-07-15.html");
        let raw = downpower_ingest::raw::RawStore::open(tmp.path()).unwrap();
        downpower_server::tests::put(&raw, downpower_ingest::raw::Series::PowerStatus, "2025-07-16", Some(&page));
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();

        let after = get(&router, "/api/barcode/attributed", &[]).await;
        assert_eq!(after.status(), StatusCode::OK);
        assert_ne!(after.headers()[ETAG], before);
    });
}

#[test]
fn api_barcode_follows_an_in_place_update() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let built = ledger::refresh(&store, tmp.path()).await.unwrap().unwrap();
        let router = router(options(), store.clone());
        let before = get(&router, "/api/barcode/attributed", &[]).await.headers()[ETAG].clone();

        // A status page for 07-16 is new content under the same version: the table updates in place.
        let page = downpower_server::tests::fixture("status/2025-07-15.html");
        let raw = downpower_ingest::raw::RawStore::open(tmp.path()).unwrap();
        downpower_server::tests::put(&raw, downpower_ingest::raw::Series::PowerStatus, "2025-07-16", Some(&page));
        let updated = ledger::refresh(&store, tmp.path()).await.unwrap().unwrap();
        assert_eq!(updated.table, built.table, "an update, not a rebuild");

        let after = get(&router, "/api/barcode/attributed", &[]).await;
        assert_eq!(after.status(), StatusCode::OK);
        assert_ne!(after.headers()[ETAG], before);
        let stale = get(&router, "/api/barcode/attributed", &[(IF_NONE_MATCH.as_str(), before.to_str().unwrap())]).await;
        assert_eq!(stale.status(), StatusCode::OK, "the old tag no longer revalidates");
    });
}

#[test]
fn api_serves_the_current_table_while_its_record_is_in_an_older_shape() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        store.query("UPDATE ledger:current UNSET summary").await.unwrap().check().unwrap();
        let table = ledger::serving(&store).await.unwrap();
        let router = router(options(), store);

        assert_eq!(get(&router, "/api/barcode/attributed", &[]).await.status(), StatusCode::OK);
        assert_eq!(get(&router, "/api/lost/attributed", &[]).await.status(), StatusCode::OK);
        assert_eq!(get(&router, "/api/unit-day/05000323/2025-07-15", &[]).await.status(), StatusCode::OK);
        // `/status` can't show the old record's summary, but says which table still serves.
        let status: downpower_app::status::Status = serde_json::from_slice(&body(get(&router, "/api/status", &[]).await).await).unwrap();
        assert_eq!((status.build, status.serving), (None, table));
    });
}

/// The log lines written while a future runs under [`Logs::subscriber`].
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Logs {
    fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync {
        tracing_subscriber::fmt().with_writer(self.clone()).with_ansi(false).finish()
    }

    fn count(&self, needle: &str) -> usize {
        String::from_utf8_lossy(&self.0.lock().unwrap()).matches(needle).count()
    }
}

impl std::io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl tracing_subscriber::fmt::MakeWriter<'_> for Logs {
    type Writer = Self;

    fn make_writer(&self) -> Self {
        self.clone()
    }
}

#[test]
fn an_older_record_is_reported_by_the_refresh_that_replaces_it_not_by_each_status_read() {
    const OLDER: &str = "ledger:current is from another version";
    let logs = Logs::default();
    let subscriber = logs.subscriber();
    block_on(
        async move {
            let tmp = tempfile::tempdir().unwrap();
            raw_store(tmp.path(), None);
            let store = db::open(tmp.path()).await.unwrap();
            ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
            store.query("UPDATE ledger:current UNSET summary").await.unwrap().check().unwrap();
            let router = router(options(), store.clone());

            for _ in 0..3 {
                assert_eq!(get(&router, "/api/status", &[]).await.status(), StatusCode::OK);
            }
            assert_eq!(logs.count(OLDER), 0, "reading /status while the rebuild runs logs nothing");
            assert!(ledger::refresh(&store, tmp.path()).await.unwrap().is_some(), "the refresh rebuilds");
            assert_eq!(logs.count(OLDER), 1);
        }
        .with_subscriber(subscriber),
    );
}

#[test]
fn api_unit_day_is_one_row_or_a_404() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let router = router(options(), store);

        let response = get(&router, "/api/unit-day/05000323/2025-07-15", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let day: UnitDay = serde_json::from_slice(&body(response).await).unwrap();
        assert_eq!((day.docket.as_str(), day.date.to_string().as_str()), ("05000323", "2025-07-15"));
        assert_ne!(day.cell(Layer::Attributed), Cell::new(Family::Unattributed, 0), "a real row");

        assert_eq!(get(&router, "/api/unit-day/05000323/1990-01-01", &[]).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(get(&router, "/api/unit-day/05000323/July", &[]).await.status(), StatusCode::BAD_REQUEST);
    });
}

#[test]
fn api_status_parses_as_the_status_page_reads_it() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let run = downpower_server::daily::run_once(&store, tmp.path(), None, "2026-11-14".parse().unwrap()).await;
        downpower_server::daily::record(&store, &run).await.unwrap();
        let router = router(options(), store);

        let response = get(&router, "/api/status", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let status: downpower_app::status::Status = serde_json::from_slice(&body(response).await).unwrap();

        let build = status.build.unwrap();
        let dates = build.summary.dates;
        let first = build.summary.first.unwrap();
        let last = build.summary.last.unwrap();
        // Every date from the first page to the last day either source covers is counted once.
        assert_eq!(
            dates.reasons + dates.held + dates.eia + dates.missing,
            u64::try_from((last - first).num_days() + 1).unwrap() + dates.missing
        );
        assert_eq!(dates.reasons, 1, "the one fixture page");
        assert_eq!(status.runs.len(), 1);
        assert!(status.runs[0].rebuilt.is_some());
        assert_eq!(status.runs[0].errors, Vec::<String>::new());
    });
}

#[test]
fn api_lost_serves_the_monthly_series() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let build = ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let router = router(options(), store);

        let response = get(&router, "/api/lost/attributed", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let monthly: downpower_core::series::Monthly = serde_json::from_slice(&body(response).await).unwrap();
        let total: u64 = monthly.lost_kwh.iter().flatten().sum();
        assert_eq!(total, build.summary.lost_kwh, "the months add up to the build's lost energy");
        assert_eq!(get(&router, "/api/lost/bogus", &[]).await.status(), StatusCode::NOT_FOUND);
    });
}
