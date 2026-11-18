//! `downpower serve`, spawned in a temporary directory over a fixture raw store (PRD-0002 uat-008,
//! PRD-0003 uat-008): it rebuilds the ledger and the event set in the background and then serves
//! the barcode buffer, the stacked area's series and page, a unit-day resolved through its event,
//! and `/status`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use std::path::Path;

use downpower_core::barcode::Barcode;
use downpower_core::series::Monthly;
use downpower_ingest::raw::{RawStore, Series};
use downpower_server::tests::{block_on, fixture, put, raw_store};

/// Kills the server when the test ends, pass or fail.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// GETs `url` until it answers 200 or `patience` runs out, and returns the body.
async fn get_ok(client: &reqwest::Client, url: &str, patience: Duration) -> Vec<u8> {
    let start = Instant::now();
    loop {
        if let Ok(response) = client.get(url).send().await
            && response.status().is_success()
        {
            return response.bytes().await.unwrap().to_vec();
        }
        assert!(start.elapsed() < patience, "{url} never answered 200");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Spawns `downpower serve` over `data_dir`, with fetching off. Returns the server and its base URL.
fn serve(data_dir: &Path) -> (Server, String) {
    // cargo-leptos copies `public/` into the site root; the icon is the one file the shell links.
    std::fs::create_dir_all(data_dir.join("site")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/favicon.svg"), data_dir.join("site/favicon.svg")).unwrap();
    let port = free_port();
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_downpower"))
            .arg("serve")
            .env("DOWNPOWER_DATA_DIR", data_dir)
            .env("LEPTOS_OUTPUT_NAME", "downpower")
            .env("LEPTOS_SITE_ROOT", data_dir.join("site"))
            .env("LEPTOS_SITE_ADDR", format!("127.0.0.1:{port}"))
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    (server, format!("http://127.0.0.1:{port}"))
}

#[test]
fn e2e_ledger_serve_rebuilds_then_serves_every_view() {
    let tmp = tempfile::tempdir().unwrap();
    raw_store(tmp.path(), None);
    let (_server, base) = serve(tmp.path());

    block_on(async move {
        let client = reqwest::Client::new();
        // The barcode answers 503 until the background rebuild lands.
        let buffer = get_ok(&client, &format!("{base}/api/barcode/attributed"), Duration::from_secs(60)).await;
        let barcode = Barcode::decode(&buffer).unwrap();
        // The fixture spans 2025-07-15 to 07-16, the page and the end of the NUC_STATUS fixture.
        assert_eq!((barcode.first().to_string(), barcode.days()), ("2025-07-15".to_owned(), 2));
        assert!(barcode.units().len() > 90, "{} rows", barcode.units().len());

        let monthly: Monthly = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/lost/attributed"), Duration::from_secs(5)).await).unwrap();
        assert_eq!((monthly.first.map(|first| first.to_string()), monthly.lost_kwh.len()), (Some("2025-07-01".to_owned()), 1));

        let home = String::from_utf8(get_ok(&client, &format!("{base}/"), Duration::from_secs(5)).await).unwrap();
        assert!(home.contains("<canvas"), "the barcode's canvas");
        assert!(home.contains("Reported power reduction"), "the stacked area");
        assert!(home.contains(r#"<link rel="icon" type="image/svg+xml" href="/favicon.svg">"#), "the icon link");
        let icon = get_ok(&client, &format!("{base}/favicon.svg"), Duration::from_secs(5)).await;
        assert!(icon.starts_with(b"<svg"), "the icon the shell links is served");

        let status = String::from_utf8(get_ok(&client, &format!("{base}/status"), Duration::from_secs(5)).await).unwrap();
        assert!(status.contains("<h1>Status</h1>"), "{status}");
        let api: downpower_app::status::Status = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/status"), Duration::from_secs(5)).await).unwrap();
        assert!(api.build.is_some());
        assert_eq!(api.runs.len(), 1, "the start-up rebuild ran once and recorded itself");
    });
}

/// The 2025-07-15 status page with Browns Ferry 3 down and citing EN 45290, its 2009 scram.
fn citing_page() -> Vec<u8> {
    let page = String::from_utf8(fixture("status/2025-07-15.html")).unwrap();
    let (head, rest) = page.split_at(page.find("<td>Browns Ferry 3</td>").unwrap());
    let rest = rest.replacen("<td>100</td>", "<td>0</td>", 1).replacen("<td>\r\n</td>", "<td>See EN #45290</td>", 1);
    format!("{head}{rest}").into_bytes()
}

#[test]
fn e2e_event_ingest_serve_resolves_a_reference_and_reports_the_archive_and_both_scram_checks() {
    let tmp = tempfile::tempdir().unwrap();
    let store: RawStore = raw_store(tmp.path(), Some(&citing_page()));
    put(&store, Series::EventNotifications, "2009-08-24", Some(&fixture("events/2009-08-24.html")));
    put(&store, Series::ScramDataset, "2026-11-14", Some(&fixture("scrams/scram-event-notifications.xlsx")));
    let (_server, base) = serve(tmp.path());

    block_on(async move {
        let client = reqwest::Client::new();
        get_ok(&client, &format!("{base}/api/barcode/attributed"), Duration::from_secs(60)).await;

        let day: serde_json::Value = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/unit-day/05000296/2025-07-15"), Duration::from_secs(5)).await).unwrap();
        assert_eq!(
            (day["power"].as_u64(), day["method"].as_str(), day["cause"].as_str()),
            (Some(0), Some("xref_en_v1"), Some("scram")),
            "{day}"
        );
        assert_eq!(day["evidence"]["event"]["number"].as_u64(), Some(45290), "{day}");

        let api: downpower_app::status::Status = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/status"), Duration::from_secs(5)).await).unwrap();
        let build = api.build.unwrap();
        let archive = build.events.unwrap();
        // The 2020-10-20 page holds six events and the 2009-08-24 page nine.
        assert_eq!((archive.pages, archive.events, archive.failed.len()), (2, 15, 0));
        assert_eq!(archive.categories.values().sum::<u64>(), 15);
        assert!(archive.categories["Power Reactor"] > 0, "{:?}", archive.categories);
        let scrams = build.summary.scrams;
        // EN 45286 and 45290 scrammed in 2009, a day no stored status page covers.
        assert_eq!(scrams.events.no_report, 2);
        let dataset = scrams.dataset.unwrap();
        assert_eq!((dataset.agree, dataset.dataset_only, dataset.ours_only, dataset.known), (0, 719, 0, 9));
    });
}

#[test]
fn e2e_explorer_serves_the_list_page_an_event_and_a_404() {
    let tmp = tempfile::tempdir().unwrap();
    let store = raw_store(tmp.path(), None);
    put(&store, Series::EventNotifications, "2009-08-24", Some(&fixture("events/2009-08-24.html")));
    let (_server, base) = serve(tmp.path());

    block_on(async move {
        let client = reqwest::Client::new();
        get_ok(&client, &format!("{base}/api/barcode/attributed"), Duration::from_secs(60)).await;

        // The page renders its filters from the URL; the list and chart load in the browser.
        let page = String::from_utf8(get_ok(&client, &format!("{base}/events?state=AL&scram=1"), Duration::from_secs(5)).await).unwrap();
        assert!(page.contains("<h1>Events</h1>"), "{page}");
        assert!(page.contains(r#"aria-pressed="true""#), "the scram toggle reads the URL");
        assert!(page.contains(r#"href="/events""#), "the nav links the explorer");

        let list: serde_json::Value = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/events?state=AL&scram=1"), Duration::from_secs(5)).await).unwrap();
        assert_eq!(list["rows"][0]["number"].as_u64(), Some(45290), "{list}");

        let shell = String::from_utf8(get_ok(&client, &format!("{base}/events/45290"), Duration::from_secs(5)).await).unwrap();
        assert!(shell.contains("downpower"), "the event page's shell");
        let event: serde_json::Value = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/events/45290"), Duration::from_secs(5)).await).unwrap();
        assert_eq!(event["units"][0]["scram"].as_str(), Some("manual_reactor"), "the unit table");
        assert!(event["updates"].is_array(), "the update timeline");
        assert!(event["nrc"].as_str().is_some_and(|nrc| nrc.ends_with("/2009/20090824en#en45290")), "{event}");

        let missing = client.get(format!("{base}/api/events/99999")).send().await.unwrap();
        assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    });
}
