//! Fixtures shared by unit and integration tests. Always compiled (not `#[cfg(test)]`), so
//! `tests/` can use it too.

use std::future::Future;
use std::path::Path;

use downpower_ingest::raw::{RawStore, Record, Series, Source};
use leptos::prelude::LeptosOptions;

/// Options for an in-process router: no site on disk, which only the static-file fallback needs.
#[must_use]
pub fn options() -> LeptosOptions {
    LeptosOptions::builder().output_name("downpower").build()
}

/// Runs `future` on a worker of [`crate::runtime`], whose stacks are deep enough for `SurrealDB` in a
/// debug build; `#[tokio::test]` runs on the 2 MiB test thread.
///
/// # Panics
///
/// When the runtime can't start or `future` panics.
#[allow(clippy::expect_used, reason = "a test fixture")]
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let runtime = crate::runtime().expect("a runtime");
    runtime.block_on(runtime.spawn(future)).expect("the test body panicked")
}

/// The fixture pages downpower-ingest keeps, read when a test asks rather than embedded in the
/// binary.
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../downpower-ingest/tests/fixtures");

/// A fixture file's bytes, e.g. `status/2025-07-15.html`.
///
/// # Panics
///
/// When the file can't be read.
#[allow(clippy::expect_used, reason = "a test fixture")]
#[must_use]
pub fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/{path}")).expect("a fixture file")
}

/// Stores `bytes` (or records a 404 for `None`) as the page for `date`, fetched 2026-11-14.
///
/// # Panics
///
/// When the store can't be written or `date` doesn't parse.
#[allow(clippy::expect_used, reason = "a test fixture")]
pub fn put(store: &RawStore, series: Series, date: &str, bytes: Option<&[u8]>) {
    let at = "2026-11-14T00:00:00Z".parse().expect("a timestamp");
    let record = Record {
        source: Source::Nrc,
        series,
        date: date.parse().expect("a date"),
        url: String::new(),
        status: if bytes.is_some() { 200 } else { 404 },
        captured_at: at,
        fetched_at: at,
        sha256: bytes.map(|bytes| store.put(bytes).expect("a stored page")),
        len: bytes.map(|bytes| bytes.len() as u64),
    };
    store.append(&record).expect("an appended record");
}

/// A raw store under `data_dir` holding `status` as the 2025-07-15 page (the real one by default),
/// a 404 for 07-16, and the `NUC_STATUS` fixture.
///
/// # Panics
///
/// When the store can't be opened or written.
#[allow(clippy::expect_used, reason = "a test fixture")]
pub fn raw_store(data_dir: &Path, status: Option<&[u8]>) -> RawStore {
    let store = RawStore::open(data_dir).expect("a raw store");
    let page = status.map_or_else(|| fixture("status/2025-07-15.html"), <[u8]>::to_vec);
    put(&store, Series::PowerStatus, "2025-07-15", Some(&page));
    put(&store, Series::PowerStatus, "2025-07-16", None);
    put(&store, Series::NucStatus, "2026-11-14", Some(&fixture("eia/nuc_status.txt")));
    store
}
