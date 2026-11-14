//! `downpower serve`, spawned in a temporary directory over a fixture raw store (PRD-0002 uat-008):
//! it rebuilds the ledger in the background and then serves the barcode buffer, the stacked area's
//! series and page, and `/status`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use downpower_core::barcode::Barcode;
use downpower_core::series::Monthly;
use downpower_server::tests::{block_on, raw_store};

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

#[test]
fn e2e_ledger_serve_rebuilds_then_serves_every_view() {
    let tmp = tempfile::tempdir().unwrap();
    raw_store(tmp.path(), None);
    let port = free_port();
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_downpower"))
            .arg("serve")
            .env("DOWNPOWER_DATA_DIR", tmp.path())
            .env("LEPTOS_OUTPUT_NAME", "downpower")
            .env("LEPTOS_SITE_ROOT", tmp.path().join("site"))
            .env("LEPTOS_SITE_ADDR", format!("127.0.0.1:{port}"))
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://127.0.0.1:{port}");

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

        let status = String::from_utf8(get_ok(&client, &format!("{base}/status"), Duration::from_secs(5)).await).unwrap();
        assert!(status.contains("<h1>Status</h1>"), "{status}");
        let api: downpower_app::status::Status = serde_json::from_slice(&get_ok(&client, &format!("{base}/api/status"), Duration::from_secs(5)).await).unwrap();
        assert!(api.build.is_some());
        assert_eq!(api.runs.len(), 1, "the start-up rebuild ran once and recorded itself");
    });
}
