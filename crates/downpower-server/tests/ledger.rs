//! The ledger in Surreal (PRD-0002 T-008, uat-004): rebuilds over a fixture raw store holding the
//! 2025-07-15 status page, a 404 for 07-16, and the `NUC_STATUS` fixture.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_ingest::events::{Event, EventSet, Scram, Site};
use downpower_ingest::facilities::{self, EventUnit, Facilities, Identity};
use downpower_ingest::ledger::{self, Source, UnitDay, Unknowns};
use downpower_ingest::raw::{Hash, RawStore, Series};
use downpower_ingest::units::Units;
use downpower_server::tests::{block_on, fixture as file, put, raw_store};
use downpower_server::{db, ledger as stored};

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn fixture(data_dir: &Path) {
    raw_store(data_dir, None);
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
    tables(store, "unit_day_").await
}

async fn tables(store: &db::Store, prefix: &str) -> Vec<String> {
    let info: Option<surrealdb::types::SerdeWrapper<serde_json::Value>> = store.query("INFO FOR DB").await.unwrap().take(0).unwrap();
    let mut tables: Vec<String> = info.unwrap().0["tables"].as_object().unwrap().keys().filter(|table| table.starts_with(prefix)).cloned().collect();
    tables.sort();
    tables
}

#[test]
fn uat_rebuild_deterministic_raw_puts_are_idempotent_and_content_addressed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();

    let status = file("status/2025-07-15.html");

    let first = store.put(&status).unwrap();
    let second = store.put(&status).unwrap();

    assert_eq!(first, second);
    assert_eq!(first, Hash::of(&status));
    assert_eq!(store.get(&first).unwrap(), status);
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
        let catalog = ledger::Catalog::embedded().unwrap();
        let events = EventSet::build(&raw, &catalog.facilities, Unknowns::Fail).unwrap();
        ledger::build(&raw, &catalog, &events, Unknowns::Fail, |days| {
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
        put(&raw, Series::PowerStatus, "2025-07-17", Some(b"<html>not a status page</html>"));

        let error = stored::rebuild(&store, tmp.path(), Unknowns::Quarantine).await.unwrap_err();

        assert!(format!("{error:#}").contains("2025-07-17"), "{error:#}");
        assert_eq!(stored::current(&store).await.unwrap(), Some(good.clone()));
        assert_eq!(build_tables(&store).await.len(), 2, "the partial table waits for the next rebuild");
        assert_ne!(rows(&store, &good.table).await, []);
    });
}

#[test]
fn ledger_refresh_rebuilds_only_for_another_version() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();

        let built = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();
        let again = stored::refresh(&store, tmp.path()).await.unwrap();
        store.query("UPDATE ledger:current SET version = 'older'").await.unwrap().check().unwrap();
        let rebuilt = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        assert_eq!(built.version, ledger::derived_version());
        assert!(again.is_none());
        assert_eq!(rebuilt.version, ledger::derived_version());
        assert_ne!(rebuilt.table, built.table);
        assert_eq!(stored::current(&store).await.unwrap(), Some(rebuilt));
    });
}

#[test]
fn ledger_refresh_rebuilds_for_new_content_and_not_for_a_refetch() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let before = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        put(&raw, Series::PowerStatus, "2025-07-15", Some(&file("status/2025-07-15.html")));
        let refetched = stored::refresh(&store, tmp.path()).await.unwrap();
        put(&raw, Series::PowerStatus, "2025-07-17", Some(&file("status/2025-07-15.html")));
        let after = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        assert!(refetched.is_none());
        assert!(after.summary.days > before.summary.days);
        assert_ne!(after.raw_digest, before.raw_digest);
    });
}

#[test]
fn ledger_a_current_record_in_an_older_shape_is_rebuilt_over() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();
        store.query("UPSERT ledger:current CONTENT { version: 'older', table: 'unit_day_1' }").await.unwrap().check().unwrap();

        let stale = stored::current(&store).await.unwrap();
        let rebuilt = stored::refresh(&store, tmp.path()).await.unwrap();

        assert_eq!(stale, None);
        assert_eq!(stored::current(&store).await.unwrap(), rebuilt);
    });
}

#[test]
fn ledger_a_current_record_in_an_older_shape_keeps_its_table_serving_until_a_rebuild_lands() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();
        let good = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        // A deploy whose `Build` no longer reads the record the last binary wrote.
        store.query("UPDATE ledger:current UNSET summary").await.unwrap().check().unwrap();
        let raw = RawStore::open(tmp.path()).unwrap();
        put(&raw, Series::PowerStatus, "2025-07-17", Some(b"<html>not a status page</html>"));

        stored::rebuild(&store, tmp.path(), Unknowns::Quarantine).await.unwrap_err();

        assert_eq!(stored::current(&store).await.unwrap(), None);
        assert_eq!(stored::serving(&store).await.unwrap(), Some(good.table.clone()));
        assert_ne!(rows(&store, &good.table).await, [], "the serving table outlives the failed rebuild");
    });
}

/// Everything a build serves: its rows, both barcodes' tags, both monthly series, the trends, the
/// FFD series and the scram series.
type Served = (
    Vec<UnitDay>,
    Vec<String>,
    Vec<downpower_core::series::Monthly>,
    downpower_core::trends::Trends,
    downpower_core::ffd::Series,
    downpower_core::triggers::Series,
);

async fn served(store: &db::Store, table: &str) -> Served {
    let mut etags = Vec::new();
    let mut series = Vec::new();
    for layer in downpower_core::barcode::Layer::ALL {
        etags.push(stored::barcode(store, table, layer).await.unwrap().unwrap().etag);
        series.push(stored::monthly(store, table, layer).await.unwrap().unwrap());
    }
    (
        rows(store, table).await,
        etags,
        series,
        stored::trends(store, table).await.unwrap().unwrap(),
        stored::ffd(store, table).await.unwrap().unwrap(),
        stored::scrams(store, table).await.unwrap().unwrap(),
    )
}

/// What a full rebuild serves over a fresh store holding the fixture plus `pages`.
async fn rebuilt_from_scratch(pages: &[(&str, Option<&[u8]>)]) -> Served {
    let tmp = tempfile::tempdir().unwrap();
    let raw = raw_store(tmp.path(), None);
    for (date, page) in pages {
        put(&raw, Series::PowerStatus, date, *page);
    }
    let store = db::open(tmp.path()).await.unwrap();
    let build = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
    served(&store, &build.table).await
}

#[test]
fn ledger_new_content_updates_the_current_table_to_what_a_full_rebuild_gives() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let first = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        // A page for 07-16, where the store had a 404: new rows in July.
        let page = file("status/2025-07-15.html");
        put(&raw, Series::PowerStatus, "2025-07-16", Some(&page));
        let updated = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();
        assert_eq!(updated.table, first.table, "updated in place");
        assert_ne!(updated.raw_digest, first.raw_digest);
        let expected = rebuilt_from_scratch(&[("2025-07-16", Some(&page))]).await;
        assert_eq!(served(&store, &updated.table).await, expected);
        assert_eq!(build_tables(&store).await, std::slice::from_ref(&first.table));

        // Both pages gone: every unit EIA doesn't report loses its July, which the update deletes.
        put(&raw, Series::PowerStatus, "2025-07-15", None);
        put(&raw, Series::PowerStatus, "2025-07-16", None);
        let emptied = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();
        let expected = rebuilt_from_scratch(&[("2025-07-16", Some(&page)), ("2025-07-15", None), ("2025-07-16", None)]).await;
        let (rows_now, ..) = served(&store, &emptied.table).await;
        assert!((rows_now.len() as u64) < first.summary.days, "{} rows", rows_now.len());
        assert_eq!(served(&store, &emptied.table).await, expected);
        assert_eq!(emptied.summary, stored::current(&store).await.unwrap().unwrap().summary);
    });
}

/// Every event the build in `table` stores, by number.
async fn events(store: &db::Store, table: &str) -> Vec<Event> {
    stored_events(store, table).await.into_iter().map(|stored| stored.event).collect()
}

async fn stored_events(store: &db::Store, table: &str) -> Vec<stored::StoredEvent> {
    let event_table = stored::event_table(table).unwrap();
    let rows: Vec<surrealdb::types::SerdeWrapper<stored::StoredEvent>> = store.query(format!("SELECT * OMIT id FROM {event_table} ORDER BY number")).await.unwrap().take(0).unwrap();
    rows.into_iter().map(|row| row.0).collect()
}

fn catalog() -> Facilities {
    Facilities::catalog(&Units::catalog().unwrap()).unwrap()
}

/// The event set a build over `data_dir`'s raw store should hold.
fn expected_events(data_dir: &Path) -> EventSet {
    EventSet::build(&RawStore::open(data_dir).unwrap(), &catalog(), Unknowns::Fail).unwrap()
}

fn all(set: &EventSet) -> Vec<Event> {
    set.events.values().cloned().collect()
}

/// Every event as a build should store it, identity included.
fn all_stored(set: &EventSet) -> Vec<stored::StoredEvent> {
    set.events
        .iter()
        .map(|(number, event)| stored::StoredEvent {
            event: event.clone(),
            identity: set.identities.get(number).cloned(),
            page: set.pages.get(number).copied(),
        })
        .collect()
}

/// Every unit row a build should store.
fn all_units(set: &EventSet) -> Vec<EventUnit> {
    set.events.iter().flat_map(|(number, event)| facilities::event_units(event, set.identities.get(number))).collect()
}

/// The fixture event page with EN 54960's licensee renamed, as the NRC re-renders an edited event.
fn edited_event_page() -> Vec<u8> {
    let page = String::from_utf8(file("events/2020-10-20.html")).unwrap();
    page.replacen("Hillis-Carnes Engineering Associates, Inc.", "Hillis-Carnes Engineering", 1).into_bytes()
}

#[test]
fn ledger_the_rebuild_stores_each_event_once() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();

        let build = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();

        let expected = expected_events(tmp.path());
        assert_eq!(stored_events(&store, &build.table).await, all_stored(&expected));
        assert_eq!(build.events, expected.archive);
        assert_eq!(build.events.events, 6);
        let one = stored::event(&store, &build.table, 54962).await.unwrap().unwrap();
        assert_eq!(one.event, expected.events[&54962]);
        assert!(matches!(one.identity, Some(Identity::Facility { docket, .. }) if docket.as_str() == "07001257"));
        assert_eq!(stored::event(&store, &build.table, 1).await.unwrap(), None);
    });
}

#[test]
fn ledger_the_event_table_swaps_and_drops_with_its_build() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let store = db::open(tmp.path()).await.unwrap();

        let first = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();
        let second = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();

        let event_table = stored::event_table(&second.table).unwrap();
        let unit_table = stored::event_unit_table(&second.table).unwrap();
        assert_eq!(event_table, second.table.replace("unit_day_", "event_"));
        assert_eq!(unit_table, second.table.replace("unit_day_", "event_unit_"));
        assert_ne!(event_table, stored::event_table(&first.table).unwrap());
        assert_eq!(tables(&store, "event_").await, [event_table, unit_table]);
        assert_eq!(events(&store, &second.table).await.len(), 6);
    });
}

#[test]
fn ledger_new_event_pages_update_the_event_table_in_place() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let first = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        put(&raw, Series::EventNotifications, "2026-02-11", Some(&file("events/2026-02-11.html")));
        let added = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        assert_eq!(added.table, first.table, "updated in place");
        assert_eq!(events(&store, &added.table).await, all(&expected_events(tmp.path())));
        assert_eq!(added.events.events, 9);

        // An edit the NRC made re-renders an event; a page gone takes its events with it.
        put(&raw, Series::EventNotifications, "2020-10-20", Some(&edited_event_page()));
        put(&raw, Series::EventNotifications, "2026-02-11", None);
        let changed = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        assert_eq!(changed.table, first.table, "updated in place");
        assert_eq!(events(&store, &changed.table).await, all(&expected_events(tmp.path())));
        assert_eq!(changed.events.events, 6);
        let edited = stored::event(&store, &changed.table, 54960).await.unwrap().unwrap();
        assert!(matches!(edited.event.site, Site::Licensee(licensee) if licensee.name.as_deref() == Some("Hillis-Carnes Engineering")));
    });
}

#[test]
fn ledger_a_broken_event_page_is_counted_by_a_refresh_and_fails_a_rebuild() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        put(&raw, Series::EventNotifications, "2020-10-21", Some(b"<html><main>not an event page</main></html>"));
        let store = db::open(tmp.path()).await.unwrap();

        let build = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();
        let error = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap_err();

        assert_eq!(build.events.failed, [day("2020-10-21")]);
        assert_eq!(events(&store, &build.table).await.len(), 6);
        assert!(format!("{error:#}").contains("the event page for 2020-10-21"), "{error:#}");
    });
}

#[test]
fn ledger_reactor_events_store_a_row_per_unit_with_its_docket() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        put(&raw, Series::EventNotifications, "2009-08-24", Some(&file("events/2009-08-24.html")));
        put(&raw, Series::EventNotifications, "2010-05-26", Some(&file("events/2010-05-26.html")));
        let store = db::open(tmp.path()).await.unwrap();

        let build = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();

        let rows = stored::event_units(&store, &build.table).await.unwrap();
        assert_eq!(rows, all_units(&expected_events(tmp.path())));
        let quad_cities: Vec<(u8, &str)> = rows.iter().filter(|row| row.event == 45285).map(|row| (row.unit, row.docket.as_str())).collect();
        assert_eq!(quad_cities, [(1, "05000254"), (2, "05000265")]);
        let vermont_yankee = rows.iter().find(|row| row.event == 45957).unwrap();
        assert_eq!(
            (vermont_yankee.docket.as_str(), vermont_yankee.occurred, vermont_yankee.scram),
            ("05000271", day("2010-05-26"), Some(Scram::Auto))
        );
        assert_eq!(rows.len(), 8);
    });
}

#[test]
fn ledger_new_event_pages_update_the_event_unit_table_in_place() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        let store = db::open(tmp.path()).await.unwrap();
        let first = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();
        assert_eq!(stored::event_units(&store, &first.table).await.unwrap(), []);

        put(&raw, Series::EventNotifications, "2009-08-24", Some(&file("events/2009-08-24.html")));
        let added = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        assert_eq!(added.table, first.table, "updated in place");
        assert_eq!(stored::event_units(&store, &added.table).await.unwrap(), all_units(&expected_events(tmp.path())));
        assert_eq!(stored_events(&store, &added.table).await, all_stored(&expected_events(tmp.path())));

        put(&raw, Series::EventNotifications, "2009-08-24", None);
        let removed = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();

        assert_eq!(removed.table, first.table, "updated in place");
        assert_eq!(stored::event_units(&store, &removed.table).await.unwrap(), []);
    });
}

#[test]
fn ledger_an_unknown_facility_is_quarantined_by_a_refresh_and_fails_a_rebuild() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let raw = raw_store(tmp.path(), None);
        let page = String::from_utf8(file("events/2020-10-20.html"))
            .unwrap()
            .replacen("Framatome Anp Richland", "Framatome Anp Kennewick", 1);
        put(&raw, Series::EventNotifications, "2020-10-20", Some(page.as_bytes()));
        let store = db::open(tmp.path()).await.unwrap();

        let build = stored::refresh(&store, tmp.path()).await.unwrap().unwrap();
        let error = stored::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap_err();

        assert_eq!(
            build.events.unknown_facilities.into_iter().collect::<Vec<_>>(),
            [("Fuel Cycle Facility \"Framatome Anp Kennewick\" in WA".to_owned(), 1)]
        );
        let quarantined = stored::event(&store, &build.table, 54962).await.unwrap().unwrap();
        assert_eq!(quarantined.identity, None);
        assert!(format!("{error:#}").contains("event 54962: unknown Fuel Cycle Facility"), "{error:#}");
    });
}
