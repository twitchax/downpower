//! The `downpower` binary, spawned in a temporary directory over a fixture raw store (PRD-0002 T-010): `rebuild`
//! builds the ledger, and a unit name the catalog doesn't know fails it (uat-006).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pretty_assertions::assert_eq;

use downpower_ingest::ledger;
use downpower_ingest::raw::RawStore;
use downpower_server::tests::{block_on, fixture, raw_store};
use downpower_server::{db, ledger as stored};

fn downpower(data_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_downpower"))
        .args(args)
        .env("DOWNPOWER_DATA_DIR", data_dir)
        .env("RUST_LOG", "info")
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn e2e_rebuild_builds_the_ledger_from_the_raw_store() {
    let tmp = tempfile::tempdir().unwrap();
    raw_store(tmp.path(), None);

    let output = downpower(tmp.path(), &["rebuild"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("rebuild done"), "{}", stderr(&output));
    let data_dir: PathBuf = tmp.path().to_owned();
    let build = block_on(async move { stored::current(&db::open(&data_dir).await.unwrap()).await.unwrap().unwrap() });
    assert_eq!(build.version, ledger::derived_version());
    assert_eq!(build.raw_digest, RawStore::open(tmp.path()).unwrap().digest().unwrap());
    assert!(build.summary.days > 90, "{} days", build.summary.days);
}

#[test]
fn uat_crosswalk_an_unknown_name_fails_the_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let page = String::from_utf8(fixture("status/2025-07-15.html")).unwrap().replace("Diablo Canyon 2", "Mystery 9");
    raw_store(tmp.path(), Some(page.as_bytes()));

    let output = downpower(tmp.path(), &["rebuild"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown unit \"Mystery 9\""), "{}", stderr(&output));
}

#[test]
fn uat_reasons_snapshot_an_unknown_tag_fails_the_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let page = String::from_utf8(fixture("status/2025-07-15.html"))
        .unwrap()
        .replace("Conservative Operations", "Zorbulating the Flux Capacitor");
    raw_store(tmp.path(), Some(page.as_bytes()));

    let output = downpower(tmp.path(), &["rebuild"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("zorbulating the flux capacitor"), "{}", stderr(&output));
}
