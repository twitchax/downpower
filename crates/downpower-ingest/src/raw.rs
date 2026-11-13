//! The raw store, the system of record (DESIGN.md Decision 15). Every fetched body is kept
//! zstd-compressed and named by the sha256 of its bytes, and every fetch is a line in an append-only
//! JSONL manifest. The database is rebuilt from this, so nothing here is ever rewritten.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{Res, Void};

/// The raw store's directory under the data dir.
pub const RAW_DIR: &str = "raw";

const BLOB_DIR: &str = "blobs";
const MANIFEST: &str = "manifest.jsonl";

/// Pages are written once and read on every rebuild, so the time goes into the write.
const ZSTD_LEVEL: i32 = 19;

/// Where a body came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Fetched live from www.nrc.gov.
    Nrc,
}

/// A series of pages, one per report date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Series {
    /// The daily Power Reactor Status Report.
    PowerStatus,
    /// The Event Notification Report, published on business days.
    EventNotifications,
}

/// The lowercase hex sha256 of a body's uncompressed bytes, which names its blob.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hash(String);

impl Hash {
    /// Hashes `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    /// The hex digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Hash {
    type Error = anyhow::Error;

    /// Only 64 lowercase hex digits pass, so a hash read back from the manifest can never name a
    /// path outside the blob dir.
    fn try_from(value: String) -> Res<Self> {
        if value.len() != 64 || !value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            bail!("not a lowercase hex sha256: {value:?}");
        }
        Ok(Self(value))
    }
}

impl From<Hash> for String {
    fn from(hash: Hash) -> Self {
        hash.0
    }
}

/// One fetch, as the manifest records it. A fetch that found no page (a 404) has no hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub source: Source,
    pub series: Series,
    /// The day the page reports on.
    pub date: NaiveDate,
    pub url: String,
    /// The HTTP status of the final response.
    pub status: u16,
    /// When the source captured the body. For a live fetch, this is the fetch time.
    pub captured_at: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
    pub sha256: Option<Hash>,
    /// The body's uncompressed length in bytes.
    pub len: Option<u64>,
}

/// The raw store under `<data dir>/raw`.
#[derive(Debug, Clone)]
pub struct RawStore {
    root: PathBuf,
}

impl RawStore {
    /// Opens the store under `data_dir`, creating it on first use. A manifest whose last line was cut
    /// off by a crash mid-append is truncated back to its last complete line.
    ///
    /// # Errors
    ///
    /// When the directories cannot be created or the manifest cannot be read or repaired.
    pub fn open(data_dir: &Path) -> Res<Self> {
        let root = data_dir.join(RAW_DIR);
        fs::create_dir_all(root.join(BLOB_DIR)).with_context(|| format!("creating the raw store at {}", root.display()))?;
        let store = Self { root };
        store.repair_torn_tail()?;
        Ok(store)
    }

    fn manifest(&self) -> PathBuf {
        self.root.join(MANIFEST)
    }

    fn blob_path(&self, hash: &Hash) -> PathBuf {
        let name = hash.as_str();
        self.root.join(BLOB_DIR).join(&name[..2]).join(format!("{name}.zst"))
    }

    fn repair_torn_tail(&self) -> Void {
        let path = self.manifest();
        let text = match fs::read(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        if text.is_empty() || text.ends_with(b"\n") {
            return Ok(());
        }
        let keep = text.iter().rposition(|&b| b == b'\n').map_or(0, |newline| newline + 1);
        tracing::warn!(path = %path.display(), dropped = text.len() - keep, "truncating a torn manifest line");
        let file = OpenOptions::new().write(true).open(&path).with_context(|| format!("opening {}", path.display()))?;
        file.set_len(u64::try_from(keep).context("manifest length")?)
            .with_context(|| format!("truncating {}", path.display()))?;
        file.sync_data().with_context(|| format!("syncing {}", path.display()))
    }

    /// Stores `bytes` and returns their hash. Storing bytes that are already present is a no-op.
    ///
    /// # Errors
    ///
    /// When compressing or writing the blob fails.
    pub fn put(&self, bytes: &[u8]) -> Res<Hash> {
        let hash = Hash::of(bytes);
        let path = self.blob_path(&hash);
        if path.exists() {
            return Ok(hash);
        }
        let dir = path.parent().context("a blob path always has a parent")?;
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let compressed = zstd::encode_all(bytes, ZSTD_LEVEL).context("compressing a blob")?;
        // Write, then rename, so a crash never leaves a partial blob under its final name.
        let tmp = path.with_extension(format!("zst.{}.tmp", std::process::id()));
        fs::write(&tmp, compressed).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming {} into place", tmp.display()))?;
        Ok(hash)
    }

    /// Reads back the bytes stored under `hash`, checking them against it.
    ///
    /// # Errors
    ///
    /// When the blob is missing, does not decompress, or does not match its hash.
    pub fn get(&self, hash: &Hash) -> Res<Vec<u8>> {
        let path = self.blob_path(hash);
        let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let bytes = zstd::decode_all(file).with_context(|| format!("decompressing {}", path.display()))?;
        if Hash::of(&bytes) != *hash {
            bail!("{} does not match its hash", path.display());
        }
        Ok(bytes)
    }

    /// Appends `record` to the manifest and syncs it to disk.
    ///
    /// # Errors
    ///
    /// When the manifest cannot be opened, written or synced.
    pub fn append(&self, record: &Record) -> Void {
        let mut line = serde_json::to_string(record).context("serializing a manifest record")?;
        line.push('\n');
        let path = self.manifest();
        let mut file = OpenOptions::new().create(true).append(true).open(&path).with_context(|| format!("opening {}", path.display()))?;
        file.write_all(line.as_bytes()).with_context(|| format!("appending to {}", path.display()))?;
        file.sync_data().with_context(|| format!("syncing {}", path.display()))
    }

    /// Every record in the manifest, oldest first.
    ///
    /// # Errors
    ///
    /// When the manifest cannot be read or a line does not parse.
    pub fn records(&self) -> Res<Vec<Record>> {
        let path = self.manifest();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        text.lines()
            .enumerate()
            .map(|(index, line)| serde_json::from_str(line).with_context(|| format!("{} line {}", path.display(), index + 1)))
            .collect()
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    fn record(date: &str, sha256: Option<Hash>) -> Record {
        let at = "2026-11-12T20:00:00Z".parse().unwrap();
        Record {
            source: Source::Nrc,
            series: Series::PowerStatus,
            date: date.parse().unwrap(),
            url: format!("https://example.test/{date}"),
            status: if sha256.is_some() { 200 } else { 404 },
            captured_at: at,
            fetched_at: at,
            len: sha256.as_ref().map(|_| 5),
            sha256,
        }
    }

    fn blob_count(data: &Path) -> usize {
        fs::read_dir(data.join(RAW_DIR).join(BLOB_DIR))
            .unwrap()
            .map(|dir| fs::read_dir(dir.unwrap().path()).unwrap().count())
            .sum()
    }

    #[test]
    fn put_is_content_addressed_and_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RawStore::open(tmp.path()).unwrap();

        let first = store.put(b"<html></html>").unwrap();
        let again = store.put(b"<html></html>").unwrap();
        let other = store.put(b"<html>other</html>").unwrap();

        assert_eq!(first, again);
        assert_eq!(first, Hash::of(b"<html></html>"));
        assert_eq!(blob_count(tmp.path()), 2);
        assert_eq!(store.get(&first).unwrap(), b"<html></html>");
        assert_eq!(store.get(&other).unwrap(), b"<html>other</html>");
    }

    #[test]
    fn get_rejects_a_blob_that_does_not_match_its_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RawStore::open(tmp.path()).unwrap();
        let hash = store.put(b"original").unwrap();
        fs::write(store.blob_path(&hash), zstd::encode_all(&b"tampered"[..], 3).unwrap()).unwrap();

        assert!(store.get(&hash).unwrap_err().to_string().contains("does not match"));
    }

    #[test]
    fn hashes_from_the_manifest_must_be_hex_digests() {
        assert!(Hash::try_from("../../etc/passwd".to_owned()).is_err());
        assert!(Hash::try_from("A".repeat(64)).is_err());
        assert!(Hash::try_from(Hash::of(b"x").as_str().to_owned()).is_ok());
    }

    #[test]
    fn manifest_round_trips_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RawStore::open(tmp.path()).unwrap();
        let stored = record("1999-01-01", Some(Hash::of(b"page")));
        let missing = record("1999-01-02", None);

        assert_eq!(store.records().unwrap(), vec![]);
        store.append(&stored).unwrap();
        store.append(&missing).unwrap();

        assert_eq!(RawStore::open(tmp.path()).unwrap().records().unwrap(), vec![stored, missing]);
    }

    #[test]
    fn open_truncates_a_torn_final_line() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RawStore::open(tmp.path()).unwrap();
        let first = record("1999-01-01", None);
        store.append(&first).unwrap();
        let mut file = OpenOptions::new().append(true).open(store.manifest()).unwrap();
        file.write_all(br#"{"source":"nrc","seri"#).unwrap();

        let reopened = RawStore::open(tmp.path()).unwrap();
        let second = record("1999-01-02", None);
        reopened.append(&second).unwrap();

        assert_eq!(reopened.records().unwrap(), vec![first, second]);
    }
}
