//! The embedded store: one `SurrealDB` database (the `SurrealKV` engine) under the data dir.
//!
//! Embedded keeps the deploy a single binary on one Fly volume. The same SDK can point at a remote
//! `SurrealDB` later without touching callers, which only ever see [`Store`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::opt::Config;

/// The env var naming the data dir: the Fly volume in production (see `fly.toml`).
pub const DATA_DIR_VAR: &str = "DOWNPOWER_DATA_DIR";

/// The data dir when [`DATA_DIR_VAR`] is unset: `./data`, which git and Docker ignore.
const DEFAULT_DATA_DIR: &str = "data";

/// The database's directory under the data dir.
pub const DB_DIR: &str = "downpower.db";

/// `SurrealKV`'s block cache and memtable caps. Uncapped, both scale with the host: the cache takes
/// half its memory less 1 GiB (about 30 GiB on the home box) and the memtable up to 1 GiB. At
/// 64 MiB each, a million-row rebuild peaks near 200 MB of RSS (PRD-0002.log.md, T-008).
const BLOCK_CACHE: u64 = 64 << 20;
const MEMTABLE: u64 = 64 << 20;

const NAMESPACE: &str = "downpower";
const DATABASE: &str = "downpower";

/// The handle every repository module takes.
pub type Store = Surreal<Db>;

/// Resolves the data dir from the value of [`DATA_DIR_VAR`]; unset or empty falls back to `./data`.
#[must_use]
pub fn data_dir(var: Option<OsString>) -> PathBuf {
    var.filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(DEFAULT_DATA_DIR), PathBuf::from)
}

/// Opens the store under `data_dir`, creating both on first use, selects downpower's namespace and
/// database, and defines the tables the repository modules read.
///
/// # Errors
///
/// When the data dir cannot be created (an unwritable volume) or the engine cannot open the
/// database.
pub async fn open(data_dir: &Path) -> anyhow::Result<Store> {
    std::fs::create_dir_all(data_dir).with_context(|| format!("creating the data dir {}", data_dir.display()))?;
    let path = data_dir.join(DB_DIR);
    let config = Config::new()
        .datastore_option("surrealkv_block_cache_capacity", BLOCK_CACHE)
        .datastore_option("surrealkv_max_memtable_size", MEMTABLE);
    let store = Surreal::new::<SurrealKv>((path.as_path(), config))
        .await
        .with_context(|| format!("opening the store at {}", path.display()))?;
    store.use_ns(NAMESPACE).use_db(DATABASE).await.context("selecting the namespace and database")?;
    crate::ledger::define(&store).await?;
    crate::daily::define(&store).await?;
    Ok(store)
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn data_dir_falls_back_when_unset_or_empty() {
        assert_eq!(data_dir(None), PathBuf::from("data"));
        assert_eq!(data_dir(Some(OsString::new())), PathBuf::from("data"));
        assert_eq!(data_dir(Some("/data".into())), PathBuf::from("/data"));
    }

    #[test]
    fn open_creates_the_store_and_round_trips_a_record() {
        crate::tests::block_on(async {
            let tmp = tempfile::tempdir().unwrap();
            // A nested, not-yet-existing dir: `open` must create it, as on a fresh volume.
            let data = tmp.path().join("fresh");

            let store = open(&data).await.unwrap();
            store.query("CREATE probe:one SET ok = true").await.unwrap().check().unwrap();
            let ok: Vec<bool> = store.query("SELECT VALUE ok FROM probe:one").await.unwrap().take(0).unwrap();

            assert_eq!(ok, vec![true]);
            assert!(data.join(DB_DIR).exists());
        });
    }
}
