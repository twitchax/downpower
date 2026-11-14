//! EIA's `NUC_STATUS` (PRD-0002 T-007), against a fixture cut from the 2026-11-14 file: Diablo
//! Canyon 2 for 2025-07-01 to 07-16, and Palisades around its 2022-05-20 shutdown, after EIA's
//! 2022–2025 hole in its series, and at the end of the file. Plus the fetch, against a fake EIA.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode, header::USER_AGENT};
use axum::response::IntoResponse;
use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::eia::{self, Day, NucStatus};
use downpower_ingest::fetch;
use downpower_ingest::raw::{Hash, RawStore, Series, Source};
use downpower_ingest::status::{self, StatusPage};
use downpower_ingest::units::{EiaGenerator, Units};

const FIXTURE: &[u8] = include_bytes!("fixtures/eia/nuc_status.txt");

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn generator(plant: u32, generator: &str) -> EiaGenerator {
    EiaGenerator { plant, generator: generator.to_owned() }
}

fn diablo_canyon_2() -> EiaGenerator {
    generator(6099, "2")
}

fn palisades() -> EiaGenerator {
    generator(1715, "1")
}

fn fixture() -> NucStatus {
    NucStatus::parse(FIXTURE).unwrap()
}

#[test]
fn eia_parse_keeps_generators_and_skips_plant_and_us_totals() {
    let nuc_status = fixture();

    let mut generators: Vec<_> = nuc_status.generators().cloned().collect();
    generators.sort_by_key(|generator| generator.plant);

    assert_eq!(generators, [palisades(), diablo_canyon_2()]);
    assert_eq!(nuc_status.end(), Some(day("2026-11-13")));
}

#[test]
fn eia_power_is_100_minus_the_percent_outage() {
    let nuc_status = fixture();

    assert_eq!(nuc_status.day(&diablo_canyon_2(), day("2025-07-05")), Some(Day { capacity_mw: 1118.0, power: 55 }));
    assert_eq!(nuc_status.day(&diablo_canyon_2(), day("2025-07-17")), None);
}

#[test]
fn eia_power_matches_the_nrc_status_page() {
    let page = status::parse(include_bytes!("fixtures/status/2025-07-15.html")).unwrap();
    let StatusPage::Report(rows) = page else { panic!("no report") };
    let row = rows.iter().find(|row| row.unit == "Diablo Canyon 2").unwrap();

    assert_eq!(fixture().day(&diablo_canyon_2(), day("2025-07-15")).unwrap().power, row.power);
}

#[test]
fn eia_capacity_carries_across_skipped_days_and_past_the_end() {
    let nuc_status = fixture();

    assert_eq!(nuc_status.capacity_mw(&palisades(), day("2023-06-01")), Some(768.5));
    assert_eq!(nuc_status.capacity_mw(&palisades(), day("2026-12-25")), Some(815.6));
    assert_eq!(nuc_status.capacity_mw(&palisades(), day("2022-05-14")), None);
    assert_eq!(nuc_status.capacity_mw(&generator(1, "1"), day("2025-07-05")), None);
}

#[test]
fn eia_gaps_fill_only_in_service_days_the_nrc_pages_lack() {
    let nuc_status = fixture();
    let units = Units::catalog().unwrap();
    let palisades = units.by_eia(&palisades()).unwrap();
    let diablo_canyon_2 = units.by_eia(&diablo_canyon_2()).unwrap();

    let missing_one: Vec<_> = nuc_status.gaps(palisades, |date| date != day("2022-05-18")).collect();
    let after_shutdown = nuc_status.gaps(palisades, |date| date <= day("2022-05-20")).count();
    let missing_all = nuc_status.gaps(diablo_canyon_2, |_| false).count();

    assert_eq!(missing_one, [(day("2022-05-18"), Day { capacity_mw: 768.5, power: 98 })]);
    assert_eq!(after_shutdown, 0);
    assert_eq!(missing_all, 16);
}

#[test]
fn uat_crosswalk_every_nuc_status_generator_maps_to_a_unit() {
    let units = Units::catalog().unwrap();
    let unknown = String::from_utf8(FIXTURE.to_vec()).unwrap().replace(".1715-1.D", ".99999-1.D");

    fixture().check(&units).unwrap();
    let error = NucStatus::parse(unknown.as_bytes()).unwrap().check(&units).unwrap_err();

    assert!(error.to_string().contains("99999"), "{error}");
}

/// A generator's two kept series, as NDJSON.
fn ndjson(capacity: &str, outage_pct: &str) -> String {
    format!("{{\"series_id\":\"NUC_STATUS.CAP.6099-2.D\",\"data\":{capacity}}}\n{{\"series_id\":\"NUC_STATUS.OUT_PCT.6099-2.D\",\"data\":{outage_pct}}}\n")
}

#[test]
fn eia_parse_accepts_a_minimal_generator() {
    let nuc_status = NucStatus::parse(ndjson(r#"[["20250705",1118]]"#, r#"[["20250705",45]]"#).as_bytes()).unwrap();

    assert_eq!(nuc_status.day(&diablo_canyon_2(), day("2025-07-05")).unwrap().power, 55);
}

#[test]
fn eia_parse_rejects_what_breaks_the_format() {
    let cases = [
        ("a fractional percent", ndjson(r#"[["20250705",1118]]"#, r#"[["20250705",45.5]]"#)),
        ("a percent over 100", ndjson(r#"[["20250705",1118]]"#, r#"[["20250705",101]]"#)),
        ("a negative capacity", ndjson(r#"[["20250705",-1]]"#, r#"[["20250705",45]]"#)),
        ("different days", ndjson(r#"[["20250705",1118]]"#, r#"[["20250706",45]]"#)),
        ("a repeated day", ndjson(r#"[["20250705",1118],["20250705",1118]]"#, r#"[["20250705",45]]"#)),
        ("a bad date", ndjson(r#"[["2025-07-05",1118]]"#, r#"[["2025-07-05",45]]"#)),
        ("no percent", ndjson(r#"[["20250705",1118]]"#, "[]").lines().next().unwrap().to_owned()),
        ("a repeated series", ndjson(r#"[["20250705",1118]]"#, r#"[["20250705",45]]"#).repeat(2)),
        ("an unknown measure", ndjson(r#"[["20250705",1118]]"#, r#"[["20250705",45]]"#).replace("OUT_PCT", "GEN")),
        ("not JSON", "{\"series_id\":".to_owned()),
    ];
    for (what, text) in cases {
        assert!(NucStatus::parse(text.as_bytes()).is_err(), "{what} parsed");
    }
}

/// A zip of `files`, deflated, as EIA serves it.
fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Serves `body` with `status` at every path, logging each request's User-Agent.
async fn fake(status: StatusCode, body: Vec<u8>) -> (String, Arc<Mutex<Vec<String>>>) {
    let agents: Arc<Mutex<Vec<String>>> = Arc::default();
    let app = Router::new().fallback({
        let agents = Arc::clone(&agents);
        move |headers: HeaderMap| {
            let agent = headers.get(USER_AGENT).map(|value| value.to_str().unwrap().to_owned()).unwrap_or_default();
            agents.lock().unwrap().push(agent);
            let body = body.clone();
            async move { (status, body).into_response() }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/opendata/bulk/NUC_STATUS.zip"), agents)
}

#[tokio::test]
async fn eia_fetch_stores_the_file_inside_the_zip() {
    let (url, agents) = fake(StatusCode::OK, zip(&[("NUC_STATUS.txt", FIXTURE)])).await;
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();

    let record = eia::fetch(&fetch::client().unwrap(), &url, &store).await.unwrap();

    assert_eq!((record.source, record.series, record.status), (Source::Eia, Series::NucStatus, 200));
    assert_eq!(record.sha256, Some(Hash::of(FIXTURE)));
    assert_eq!(record.len, Some(FIXTURE.len() as u64));
    assert_eq!(store.get(&Hash::of(FIXTURE)).unwrap(), FIXTURE);
    assert_eq!(store.latest(Series::NucStatus).unwrap().into_values().collect::<Vec<_>>(), [record]);
    assert_eq!(*agents.lock().unwrap(), [fetch::USER_AGENT]);
}

#[tokio::test]
async fn eia_fetch_stores_nothing_from_a_failed_response_or_an_unexpected_zip() {
    let cases = [
        (StatusCode::NOT_FOUND, b"no such file".to_vec()),
        (StatusCode::OK, b"<html>maintenance</html>".to_vec()),
        (StatusCode::OK, zip(&[("NUC_STATUS.txt", FIXTURE), ("README.txt", b"hi")])),
    ];
    for (status, body) in cases {
        let (url, _) = fake(status, body).await;
        let tmp = tempfile::tempdir().unwrap();
        let store = RawStore::open(tmp.path()).unwrap();

        assert!(eia::fetch(&fetch::client().unwrap(), &url, &store).await.is_err(), "{status}");
        assert!(store.records().unwrap().is_empty(), "{status}");
    }
}
