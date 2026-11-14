//! The ledger in Surreal (PRD-0002 T-008, uat-004): rebuilds over a fixture raw store holding the
//! 2025-07-15 status page, a 404 for 07-16, and the `NUC_STATUS` fixture.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::ledger::{self, Source, UnitDay, Unknowns};
use downpower_ingest::raw::{self, Hash, RawStore, Record, Series};
use downpower_ingest::units::Units;
use downpower_server::tests::block_on;
use downpower_server::{db, ledger as stored};

const STATUS: &[u8] = include_bytes!("../../downpower-ingest/tests/fixtures/status/2025-07-15.html");
const NUC_STATUS: &[u8] = include_bytes!("../../downpower-ingest/tests/fixtures/eia/nuc_status.txt");

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn record(store: &RawStore, series: Series, date: &str, bytes: Option<&[u8]>) -> Record {
    let at = "2026-11-14T00:00:00Z".parse().unwrap();
    Record {
        source: raw::Source::Nrc,
        series,
        date: day(date),
        url: String::new(),
        status: if bytes.is_some() { 200 } else { 404 },
        captured_at: at,
        fetched_at: at,
        sha256: bytes.map(|bytes| store.put(bytes).unwrap()),
        len: bytes.map(|bytes| bytes.len() as u64),
    }
}

fn fixture(data_dir: &Path) {
    let store = RawStore::open(data_dir).unwrap();
    store.append(&record(&store, Series::PowerStatus, "2025-07-15", Some(STATUS))).unwrap();
    store.append(&record(&store, Series::PowerStatus, "2025-07-16", None)).unwrap();
    store.append(&record(&store, Series::NucStatus, "2026-11-14", Some(NUC_STATUS))).unwrap();
}

/// Every row of `table`, unit by unit.
async fn rows(store: &db::Store, table: &str) -> Vec<UnitDay> {
    let mut rows = Vec::new();
    for unit in Units::catalog().unwrap().all() {
        rows.extend(stored::unit_days(store, table, unit.docket.as_str()).await.unwrap());
    }
    rows
}

async fn build_tables(store: &db::Store) -> Vec<String> {
    let info: Option<surrealdb::types::SerdeWrapper<serde_json::Value>> = store.query("INFO FOR DB").await.unwrap().take(0).unwrap();
    let mut tables: Vec<String> = info.unwrap().0["tables"].as_object().unwrap().keys().filter(|table| table.starts_with("unit_day_")).cloned().collect();
    tables.sort();
    tables
}

#[test]
fn uat_rebuild_deterministic_raw_puts_are_idempotent_and_content_addressed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();

    let first = store.put(STATUS).unwrap();
    let second = store.put(STATUS).unwrap();

    assert_eq!(first, second);
    assert_eq!(first, Hash::of(STATUS));
    assert_eq!(store.get(&first).unwrap(), STATUS);
    let blobs = walk(&tmp.path().join("raw/blobs"));
    assert_eq!(blobs, [format!("{}/{}.zst", &first.as_str()[..2], first.as_str())]);
}

fn walk(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path).into_iter().map(|file| format!("{}/{file}", path.file_name().unwrap().to_str().unwrap())));
        } else {
            files.push(path.file_name().unwrap().to_str().unwrap().to_owned());
        }
    }
    files
}

#[test]
fn uat_rebuild_deterministic_twice_from_one_raw_store_gives_identical_rows() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();

        let first = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let first_rows = rows(&store, &first.table).await;
        let second = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let second_rows = rows(&store, &second.table).await;

        assert_eq!(first_rows, second_rows);
        assert_eq!(first.summary, second.summary);
        assert_ne!(first.table, second.table);
        assert_eq!(first_rows.len() as u64, first.summary.days);
        assert!(first_rows.len() > 90, "{} rows", first_rows.len());
        assert_eq!(build_tables(&store).await, std::slice::from_ref(&second.table), "the first build's table is dropped");
        assert_eq!(stored::current(&store).await.unwrap(), Some(second));
    });
}

#[test]
fn ledger_rows_read_back_as_written() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();
        let mut in_memory = Vec::new();
        let raw = RawStore::open(tmp.path()).unwrap();
        let (tags, units) = (downpower_ingest::tags::Tags::catalog().unwrap(), Units::catalog().unwrap());
        ledger::build(&raw, &tags, &units, Unknowns::Fail, |days| {
            in_memory.extend(days);
            Ok(())
        })
        .unwrap();

        let current = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let diablo_canyon_2 = stored::unit_days(&store, &current.table, "05000323").await.unwrap();

        let expected: Vec<_> = in_memory.into_iter().filter(|day| day.docket == "05000323").collect();
        assert_eq!(diablo_canyon_2, expected);
        assert_eq!(
            diablo_canyon_2.iter().map(|day| (day.date, day.source)).collect::<Vec<_>>(),
            [(day("2025-07-15"), Source::NrcPage), (day("2025-07-16"), Source::Eia)]
        );
    });
}

#[test]
fn ledger_the_rebuild_writes_the_unit_table() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();

        stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let names: Vec<String> = store.query("SELECT VALUE name FROM unit:⟨05000323⟩").await.unwrap().take(0).unwrap();
        let count: Option<usize> = store.query("SELECT VALUE count() FROM unit GROUP ALL").await.unwrap().take(0).unwrap();

        assert_eq!(names, ["Diablo Canyon 2"]);
        assert_eq!(count, Some(Units::catalog().unwrap().all().len()));
    });
}

#[test]
fn ledger_a_failed_rebuild_leaves_the_previous_build_current() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();
        let good = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let raw = RawStore::open(tmp.path()).unwrap();
        raw.append(&record(&raw, Series::PowerStatus, "2025-07-17", Some(b"<html>not a status page</html>"))).unwrap();

        let error = stored::rebuild(&store, tmp.path(), Unknowns::Quarantine).await.unwrap_err();

        assert!(format!("{error:#}").contains("2025-07-17"), "{error:#}");
        assert_eq!(stored::current(&store).await.unwrap(), Some(good.clone()));
        assert_eq!(build_tables(&store).await.len(), 2, "the partial table waits for the next rebuild");
        assert_ne!(rows(&store, &good.table).await, []);
    });
}

#[test]
fn ledger_ensure_current_rebuilds_only_for_another_version() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();

        stored::ensure_current(&store, tmp.path()).await.unwrap().unwrap().await.unwrap();
        let built = stored::current(&store).await.unwrap().unwrap();
        let again = stored::ensure_current(&store, tmp.path()).await.unwrap();
        store.query("UPDATE ledger:current SET version = 'older'").await.unwrap().check().unwrap();
        stored::ensure_current(&store, tmp.path()).await.unwrap().unwrap().await.unwrap();

        assert_eq!(built.version, ledger::derived_version());
        assert!(again.is_none());
        let rebuilt = stored::current(&store).await.unwrap().unwrap();
        assert_eq!(rebuilt.version, ledger::derived_version());
        assert_ne!(rebuilt.table, built.table);
    });
}
