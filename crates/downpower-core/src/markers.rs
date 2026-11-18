//! The barcode's event markers (PRD-0004 T-006, DESIGN.md Decision 35): each unit of every mapped,
//! unretracted reactor event, packed for the browser to draw over the barcode. Markers name their
//! unit by docket, through a table at the head of the buffer, so they don't depend on the barcode's
//! row order.
//!
//! The buffer: `DPM1`; a `u16` count of dockets, each a `u8` length and its bytes; a `u32` count of
//! markers, each a `u16` docket index, a `u16` day counted from [`EPOCH`], a `u8` of flags (bit 0, a
//! scram) and the `u32` event number. Little-endian throughout.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use anyhow::{Context as _, bail, ensure};
use chrono::NaiveDate;

use crate::Res;

/// Day zero of the markers' dates: the first day either NRC series covers.
pub const EPOCH: NaiveDate = match NaiveDate::from_ymd_opt(1999, 1, 1) {
    Some(date) => date,
    None => panic!("1999-01-01 is a valid date"),
};

const MAGIC: &[u8; 4] = b"DPM1";
const SCRAM: u8 = 1;

/// One event's mark on one unit-day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Marker {
    /// The unit, as an index into [`Markers::dockets`].
    pub unit: u16,
    pub date: NaiveDate,
    pub number: u32,
    /// Whether the event gives the unit a SCRAM Code other than `N`.
    pub scram: bool,
}

/// Every marker, by unit, date and number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Markers {
    dockets: Vec<String>,
    markers: Vec<Marker>,
}

/// One unit of an event: its docket, the event date, its number and whether it scrammed the unit.
pub type Entry = (String, NaiveDate, u32, bool);

impl Markers {
    /// The markers of `entries`, dropping any dated before [`EPOCH`] or past the `u16` days after it.
    ///
    /// # Errors
    ///
    /// When there are more than `u16::MAX` dockets.
    pub fn new(entries: impl IntoIterator<Item = Entry>) -> Res<Self> {
        let mut by_docket: BTreeMap<String, Vec<(NaiveDate, u32, bool)>> = BTreeMap::new();
        for (docket, date, number, scram) in entries {
            if day(date).is_some() {
                by_docket.entry(docket).or_default().push((date, number, scram));
            }
        }
        let mut markers = Vec::new();
        for (index, rows) in by_docket.values().enumerate() {
            let unit = u16::try_from(index).context("more than 65,535 units have events")?;
            markers.extend(rows.iter().map(|(date, number, scram)| Marker {
                unit,
                date: *date,
                number: *number,
                scram: *scram,
            }));
        }
        markers.sort();
        markers.dedup();
        Ok(Self {
            dockets: by_docket.into_keys().collect(),
            markers,
        })
    }

    #[must_use]
    pub fn dockets(&self) -> &[String] {
        &self.dockets
    }

    #[must_use]
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// The markers on `docket` within `dates`, by date and number.
    #[must_use]
    pub fn at(&self, docket: &str, dates: RangeInclusive<NaiveDate>) -> &[Marker] {
        let Ok(unit) = self.dockets.binary_search_by(|candidate| candidate.as_str().cmp(docket)) else {
            return &[];
        };
        let Ok(unit) = u16::try_from(unit) else { return &[] };
        let start = self.markers.partition_point(|marker| (marker.unit, marker.date) < (unit, *dates.start()));
        let end = self.markers.partition_point(|marker| (marker.unit, marker.date) <= (unit, *dates.end()));
        &self.markers[start..end.max(start)]
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + 2 + self.dockets.len() * 9 + 4 + self.markers.len() * 9);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&u16::try_from(self.dockets.len()).unwrap_or(u16::MAX).to_le_bytes());
        for docket in &self.dockets {
            bytes.push(u8::try_from(docket.len()).unwrap_or(u8::MAX));
            bytes.extend_from_slice(docket.as_bytes());
        }
        bytes.extend_from_slice(&u32::try_from(self.markers.len()).unwrap_or(u32::MAX).to_le_bytes());
        for marker in &self.markers {
            bytes.extend_from_slice(&marker.unit.to_le_bytes());
            bytes.extend_from_slice(&day(marker.date).unwrap_or_default().to_le_bytes());
            bytes.push(if marker.scram { SCRAM } else { 0 });
            bytes.extend_from_slice(&marker.number.to_le_bytes());
        }
        bytes
    }

    /// # Errors
    ///
    /// When `bytes` isn't a marker buffer, or is cut short.
    pub fn decode(bytes: &[u8]) -> Res<Self> {
        let mut reader = Reader { bytes, at: 0 };
        ensure!(reader.take(4)? == MAGIC, "not a marker buffer");
        let count = usize::from(u16::from_le_bytes(reader.array()?));
        let mut dockets = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::from(reader.take(1)?[0]);
            dockets.push(String::from_utf8(reader.take(len)?.to_vec()).context("a docket that isn't UTF-8")?);
        }
        let count = usize::try_from(u32::from_le_bytes(reader.array()?)).context("a marker count")?;
        let mut markers = Vec::with_capacity(count.min(bytes.len() / 9));
        for _ in 0..count {
            let unit = u16::from_le_bytes(reader.array()?);
            let offset = u16::from_le_bytes(reader.array()?);
            let flags = reader.take(1)?[0];
            let number = u32::from_le_bytes(reader.array()?);
            ensure!(usize::from(unit) < dockets.len(), "a marker names docket {unit} of {}", dockets.len());
            let date = EPOCH.checked_add_days(chrono::Days::new(u64::from(offset))).context("a marker date")?;
            markers.push(Marker {
                unit,
                date,
                number,
                scram: flags & SCRAM != 0,
            });
        }
        ensure!(reader.at == bytes.len(), "{} bytes after the markers", bytes.len() - reader.at);
        Ok(Self { dockets, markers })
    }
}

/// Days from [`EPOCH`] to `date`, when they fit a `u16`.
fn day(date: NaiveDate) -> Option<u16> {
    u16::try_from((date - EPOCH).num_days()).ok()
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Res<&'a [u8]> {
        let Some(slice) = self.bytes.get(self.at..self.at + len) else {
            bail!("the marker buffer is cut short at byte {}", self.at);
        };
        self.at += len;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Res<[u8; N]> {
        self.take(N)?.try_into().context("a fixed-width field")
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    fn entries() -> Vec<Entry> {
        vec![
            ("05000296".to_owned(), day("2009-08-24"), 45290, true),
            ("05000260".to_owned(), day("1999-01-15"), 35269, false),
            ("05000296".to_owned(), day("1999-01-15"), 35269, false),
            ("05000296".to_owned(), day("2009-08-24"), 45291, false),
            ("05000302".to_owned(), day("1998-12-31"), 1, true),
        ]
    }

    #[test]
    fn uat_event_markers_round_trip_through_the_buffer() {
        let markers = Markers::new(entries()).unwrap_or_default();

        assert_eq!(markers.dockets(), ["05000260", "05000296"], "a marker before 1999 is dropped, and its docket with it");
        assert_eq!(markers.markers().len(), 4);
        assert_eq!(Markers::decode(&markers.encode()).ok(), Some(markers));
    }

    #[test]
    fn uat_event_markers_are_found_by_unit_and_days() {
        let markers = Markers::new(entries()).unwrap_or_default();

        let found = markers.at("05000296", day("2009-08-24")..=day("2009-08-24"));
        assert_eq!(found.iter().map(|marker| (marker.number, marker.scram)).collect::<Vec<_>>(), [(45290, true), (45291, false)]);
        assert_eq!(markers.at("05000296", day("1999-01-01")..=day("2009-08-23")).len(), 1);
        assert_eq!(markers.at("05000999", day("1999-01-01")..=day("2026-01-01")).len(), 0);
    }

    #[test]
    fn a_damaged_buffer_is_refused() {
        let bytes = Markers::new(entries()).unwrap_or_default().encode();
        assert!(Markers::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(Markers::decode(b"DPM2").is_err());
        let mut long = bytes;
        long.push(0);
        assert!(Markers::decode(&long).is_err());
    }
}
