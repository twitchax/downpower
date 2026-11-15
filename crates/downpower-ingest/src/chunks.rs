//! A unit's rows a month at a time, each month hashed, so a rebuild whose inputs moved only a little
//! rewrites only the months that changed (PRD-0002.log.md, T-019). Writing every row of every build
//! was most of a rebuild's time on Fly's throttled CPU.

use std::collections::BTreeMap;
use std::hash::{Hash as _, Hasher};

use anyhow::{Context as _, ensure};
use chrono::{Datelike as _, NaiveDate};

use crate::Res;
use crate::ledger::UnitDay;

/// The first day of `date`'s month.
#[must_use]
pub fn month_of(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

/// One unit's rows for one month, and their hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub docket: String,
    /// The first day of the month.
    pub month: NaiveDate,
    pub hash: u64,
    pub days: Vec<UnitDay>,
}

/// 64-bit FNV-1a: small, fast, and the same in every build, so hashes stored by one binary compare
/// with the next. (A change in how a type feeds a hasher only costs one full rewrite.)
struct Fnv(u64);

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// The hash of `days`, in order.
#[must_use]
pub fn hash(days: &[UnitDay]) -> u64 {
    let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
    days.hash(&mut hasher);
    hasher.finish()
}

/// Gathers rows, which arrive a date at a time in date order, into per-unit months.
#[derive(Debug, Default)]
pub struct Chunker {
    month: Option<NaiveDate>,
    open: BTreeMap<String, Vec<UnitDay>>,
}

impl Chunker {
    /// Takes one date's rows, and returns the previous month's chunks once `days` starts a new one.
    pub fn push(&mut self, days: Vec<UnitDay>) -> Vec<Chunk> {
        let Some(date) = days.first().map(|day| day.date) else { return Vec::new() };
        let month = month_of(date);
        let done = if self.month.is_some_and(|open| open != month) { self.close() } else { Vec::new() };
        self.month = Some(month);
        for day in days {
            self.open.entry(day.docket.clone()).or_default().push(day);
        }
        done
    }

    /// The chunks still open.
    #[must_use]
    pub fn finish(mut self) -> Vec<Chunk> {
        self.close()
    }

    fn close(&mut self) -> Vec<Chunk> {
        let Some(month) = self.month else { return Vec::new() };
        std::mem::take(&mut self.open)
            .into_iter()
            .map(|(docket, days)| Chunk { hash: hash(&days), docket, month, days })
            .collect()
    }
}

/// Every chunk's hash in a build, by docket and month.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hashes(pub BTreeMap<(String, NaiveDate), u64>);

/// Bytes per entry: the docket, the month as `yyyymm`, and the hash.
const ENTRY: usize = 8 + 4 + 8;

impl Hashes {
    /// Packed for storage: per chunk, the 8-byte docket, the month as a little-endian `yyyymm`,
    /// and the hash, little-endian.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.0.len() * ENTRY);
        for ((docket, month), hash) in &self.0 {
            let mut padded = [b' '; 8];
            for (slot, byte) in padded.iter_mut().zip(docket.bytes()) {
                *slot = byte;
            }
            bytes.extend_from_slice(&padded);
            let yyyymm = u32::try_from(month.year()).unwrap_or(0) * 100 + month.month();
            bytes.extend_from_slice(&yyyymm.to_le_bytes());
            bytes.extend_from_slice(&hash.to_le_bytes());
        }
        bytes
    }

    /// # Errors
    ///
    /// When `bytes` isn't a whole number of entries or holds a month or docket that doesn't parse.
    pub fn decode(bytes: &[u8]) -> Res<Self> {
        ensure!(bytes.len().is_multiple_of(ENTRY), "{} bytes isn't a whole number of chunk hashes", bytes.len());
        let mut hashes = BTreeMap::new();
        for entry in bytes.as_chunks::<ENTRY>().0 {
            let (docket, rest) = entry.split_at(8);
            let (yyyymm, hash) = rest.split_at(4);
            let docket = std::str::from_utf8(docket).context("a chunk's docket isn't UTF-8")?.trim_end().to_owned();
            let yyyymm = u32::from_le_bytes(yyyymm.try_into()?);
            let month = NaiveDate::from_ymd_opt(i32::try_from(yyyymm / 100)?, yyyymm % 100, 1).with_context(|| format!("month {yyyymm}"))?;
            hashes.insert((docket, month), u64::from_le_bytes(hash.try_into()?));
        }
        Ok(Self(hashes))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::attribution::{Evidence, Method};
    use crate::ledger::Source;
    use crate::tags::Family;

    fn day(docket: &str, date: &str, power: u8) -> UnitDay {
        UnitDay {
            docket: docket.to_owned(),
            date: date.parse().unwrap_or_default(),
            power,
            source: Source::NrcPage,
            eia_power: None,
            reason: None,
            down: None,
            changed: false,
            scrams: 0,
            causes: Vec::new(),
            flags: Vec::new(),
            transition: false,
            xrefs: Vec::new(),
            unclassified: Vec::new(),
            unknown: Vec::new(),
            cause: None,
            family: Family::Unattributed,
            method: Method::NoCause,
            evidence: Evidence::None,
            capacity_kw: None,
            lost_kwh: None,
        }
    }

    #[test]
    fn rows_gather_into_unit_months_as_dates_pass() {
        let mut chunker = Chunker::default();
        assert_eq!(chunker.push(vec![day("05000275", "2024-01-30", 100), day("05000323", "2024-01-30", 90)]), []);
        assert_eq!(chunker.push(vec![day("05000275", "2024-01-31", 100)]), []);
        let january = chunker.push(vec![day("05000275", "2024-02-01", 50)]);
        assert_eq!(
            january.iter().map(|chunk| (chunk.docket.as_str(), chunk.days.len())).collect::<Vec<_>>(),
            [("05000275", 2), ("05000323", 1)]
        );
        assert!(january.iter().all(|chunk| chunk.month.to_string() == "2024-01-01"));
        let february = chunker.finish();
        assert_eq!((february.len(), february[0].month.to_string()), (1, "2024-02-01".to_owned()));
    }

    #[test]
    fn a_hash_moves_with_any_field_and_holds_otherwise() {
        let days = [day("05000275", "2024-01-30", 100), day("05000275", "2024-01-31", 100)];
        assert_eq!(hash(&days), hash(&days.clone()));
        let mut changed = days.clone();
        changed[1].reason = Some("Refueling Outage".to_owned());
        assert_ne!(hash(&changed), hash(&days));
        assert_ne!(hash(&days[..1]), hash(&days));
    }

    #[test]
    fn hashes_round_trip_through_their_bytes() -> Res<()> {
        let mut hashes = Hashes::default();
        hashes.0.insert(("05000275".to_owned(), "1999-01-01".parse()?), u64::MAX);
        hashes.0.insert(("05000323".to_owned(), "2026-11-01".parse()?), 7);
        let bytes = hashes.encode();
        assert_eq!(bytes.len(), 2 * ENTRY);
        assert_eq!(Hashes::decode(&bytes)?, hashes);
        assert!(Hashes::decode(&bytes[1..]).is_err());
        Ok(())
    }
}
