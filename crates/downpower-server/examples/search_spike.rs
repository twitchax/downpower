//! PRD-0004 T-001: `SurrealDB`'s full-text index against an in-memory scan, over every stored event's
//! title, text and names. Builds the event set from the raw store under `$DOWNPOWER_DATA_DIR`
//! (default `./data`), indexes it in a scratch database, and prints the index's write time, peak
//! RSS and size on disk, then the median latency of each query both ways.
//!
//! `cargo run --release -p downpower-server --example search_spike`; prefix `taskset -c 0` for one
//! core.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use downpower_ingest::events::{EventSet, Site};
use downpower_ingest::ledger::{Catalog, Unknowns};
use downpower_ingest::raw::RawStore;
use serde::{Deserialize, Serialize};
use surrealdb::types::SerdeWrapper;

/// The queries timed both ways: single words, a phrase-like pair, a name and a 10 CFR code.
const QUERIES: [&str; 8] = [
    "scram",
    "tritium",
    "diesel generator",
    "feedwater pump",
    "fitness for duty",
    "turbine trip",
    "palo verde",
    "overexposure",
];

/// Runs per query; the median is reported.
const RUNS: usize = 5;

/// Rows per insert, as the rebuild batches events.
const BATCH: usize = 500;

#[derive(Clone, Serialize, Deserialize)]
struct Doc {
    id: u32,
    body: String,
}

/// An event's searchable text: its title, text, site or licensee names, and persons, lowercased.
fn body(event: &downpower_ingest::events::Event) -> String {
    let mut parts: Vec<&str> = vec![event.title.as_deref().unwrap_or_default(), &event.text];
    match &event.site {
        Site::Facility(facility) => parts.push(&facility.name),
        Site::Licensee(licensee) => parts.extend([licensee.rep_org.as_deref(), licensee.name.as_deref(), licensee.city.as_deref()].into_iter().flatten()),
    }
    parts.extend(event.persons.iter().map(String::as_str));
    parts.join("\n").to_lowercase()
}

/// `VmRSS` and `VmHWM` in MB.
fn memory() -> anyhow::Result<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").context("reading /proc/self/status")?;
    let field = |name: &str| -> anyhow::Result<u64> {
        let line = status.lines().find(|line| line.starts_with(name)).with_context(|| format!("no {name}"))?;
        let kb: u64 = line.split_whitespace().nth(1).context("no value")?.parse()?;
        Ok(kb / 1024)
    };
    Ok((field("VmRSS:")?, field("VmHWM:")?))
}

/// Resets `VmHWM` to the current RSS, so the next peak is the next phase's.
fn reset_peak() -> anyhow::Result<()> {
    std::fs::write("/proc/self/clear_refs", "5").context("resetting VmHWM")
}

fn dir_size(path: &Path) -> u64 {
    walk(path)
        .iter()
        .filter_map(|entry| entry.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|meta| meta.len())
        .sum()
}

fn walk(path: &Path) -> Vec<std::fs::DirEntry> {
    let Ok(entries) = std::fs::read_dir(path) else { return Vec::new() };
    entries
        .flatten()
        .flat_map(|entry| {
            let nested = if entry.path().is_dir() { walk(&entry.path()) } else { Vec::new() };
            std::iter::once(entry).chain(nested)
        })
        .collect()
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let data_dir = downpower_server::db::data_dir(std::env::var_os(downpower_server::db::DATA_DIR_VAR));
    let started = Instant::now();
    let set = EventSet::build(&RawStore::open(&data_dir)?, &Catalog::embedded()?.facilities, Unknowns::Quarantine)?;
    let docs: Vec<Doc> = set.events.values().map(|event| Doc { id: event.number, body: body(event) }).collect();
    drop(set);
    let bytes: usize = docs.iter().map(|doc| doc.body.len()).sum();
    println!("{} events, {} MB of text, built in {:.1} s", docs.len(), bytes >> 20, started.elapsed().as_secs_f64());

    reset_peak()?;
    let (rss, _) = memory()?;
    let scratch = tempfile::tempdir()?;
    let store = downpower_server::db::open(scratch.path()).await?;
    // `SPIKE_NO_INDEX` writes the same rows without the index, to separate its cost from the store's.
    let indexed = std::env::var_os("SPIKE_NO_INDEX").is_none();
    let schema = if indexed {
        "DEFINE ANALYZER words TOKENIZERS blank,class,punct FILTERS lowercase,ascii; DEFINE TABLE ev SCHEMALESS; DEFINE INDEX ev_body ON ev FIELDS body FULLTEXT ANALYZER words BM25;"
    } else {
        "DEFINE TABLE ev SCHEMALESS;"
    };
    store.query(schema).await?.check()?;
    let started = Instant::now();
    for batch in docs.chunks(BATCH) {
        store.query("INSERT INTO ev $rows RETURN NONE").bind(("rows", SerdeWrapper(batch.to_vec()))).await?.check()?;
    }
    let written = started.elapsed();
    let (after, peak) = memory()?;
    println!(
        "index: written in {:.1} s; RSS {rss} MB before, {after} MB after, peak {peak} MB; {} MB on disk",
        written.as_secs_f64(),
        dir_size(scratch.path()) >> 20
    );

    if !indexed {
        return Ok(());
    }
    for query in QUERIES {
        let mut indexed = Vec::new();
        let mut hits = 0;
        for _ in 0..RUNS {
            let started = Instant::now();
            let ids: Vec<SerdeWrapper<serde_json::Value>> = store.query("SELECT VALUE id FROM ev WHERE body @@ $q").bind(("q", query)).await?.take(0)?;
            indexed.push(started.elapsed());
            hits = ids.len();
        }
        let mut scanned = Vec::new();
        let mut found = 0;
        for _ in 0..RUNS {
            let started = Instant::now();
            found = docs.iter().filter(|doc| query.split(' ').all(|word| doc.body.contains(word))).count();
            scanned.push(started.elapsed());
        }
        println!(
            "{query:<18} index {:>7.2} ms ({hits:>5} hits)   scan {:>7.2} ms ({found:>5} hits)",
            median(indexed).as_secs_f64() * 1e3,
            median(scanned).as_secs_f64() * 1e3
        );
    }
    Ok(())
}
