//! The barcode buffer (DESIGN.md Decision 23): every unit-day of the fleet as one byte, so the
//! browser draws 25 years of a hundred-odd reactors from about a megabyte. A cell's high three bits
//! are its family and its low five a power bucket. [`NO_DATA`] marks a day with no row: the unit was
//! out of service, or nothing reported it.
//!
//! On the wire: the magic `DPB1`; the first day as days from the common era (`i32`), the day count
//! (`u32`) and the row count (`u16`), all little-endian; per row, its docket and name, each a length
//! byte and UTF-8; then the cells, row-major, one row per unit.

use std::ops::{Range, RangeInclusive};

use anyhow::{Context as _, anyhow, bail, ensure};
use chrono::{Datelike as _, Days, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::{Families, Family, Res};

pub const MAGIC: [u8; 4] = *b"DPB1";

/// The byte for a day with no row.
pub const NO_DATA: u8 = u8::MAX;

/// The bucket 100% power has to itself. Powers from 0 to 99% share the buckets below it, about
/// 3.3 points each.
pub const FULL: u8 = 30;

const BUCKET_BITS: u8 = 5;
const BUCKET_MASK: u8 = (1 << BUCKET_BITS) - 1;

/// Which of a unit-day's two labels a cell carries (Decision 20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// The attributed cause, inherited and inferred causes included.
    #[default]
    Attributed,
    /// Only a cause the day's own reason names. Inherited, cross-referenced and pending days count
    /// as Unattributed.
    Observed,
}

impl Layer {
    pub const ALL: [Self; 2] = [Self::Attributed, Self::Observed];

    /// The name in URLs and API paths.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Attributed => "attributed",
            Self::Observed => "observed",
        }
    }

    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.slug() == slug)
    }
}

/// The bucket for `power` percent.
#[must_use]
pub fn bucket(power: u8) -> u8 {
    if power >= 100 {
        return FULL;
    }
    u8::try_from(u16::from(power) * u16::from(FULL) / 100).unwrap_or(FULL - 1)
}

/// One unit-day: its family and its power bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub family: Family,
    pub bucket: u8,
}

impl Cell {
    /// A day at `power` percent, labeled `family`.
    #[must_use]
    pub fn new(family: Family, power: u8) -> Self {
        Self { family, bucket: bucket(power) }
    }

    #[must_use]
    pub const fn pack(self) -> u8 {
        (self.family.index() << BUCKET_BITS) | self.bucket
    }

    /// The cell `byte` holds, or `None` for [`NO_DATA`] and any other byte whose bucket is out of
    /// range.
    #[must_use]
    pub fn unpack(byte: u8) -> Option<Self> {
        let bucket = byte & BUCKET_MASK;
        if bucket > FULL {
            return None;
        }
        Some(Self {
            family: Family::from_index(byte >> BUCKET_BITS)?,
            bucket,
        })
    }

    /// The powers the bucket covers.
    #[must_use]
    pub fn power(self) -> RangeInclusive<u8> {
        if self.bucket >= FULL {
            return 100..=100;
        }
        // Bucket b holds the powers p with ⌊30p / 100⌋ = b.
        let low = (u16::from(self.bucket) * 100).div_ceil(u16::from(FULL));
        let high = (u16::from(self.bucket + 1) * 100).div_ceil(u16::from(FULL)) - 1;
        u8::try_from(low).unwrap_or(0)..=u8::try_from(high).unwrap_or(99)
    }

    /// How far below full power the bucket's middle sits, from 0 to 1.
    #[must_use]
    pub fn reduction(self) -> f32 {
        let power = self.power();
        let middle = f32::midpoint(f32::from(*power.start()), f32::from(*power.end()));
        (100.0 - middle) / 100.0
    }
}

/// A row's unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub docket: String,
    pub name: String,
}

/// The decoded buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Barcode {
    first: NaiveDate,
    days: u32,
    units: Vec<Unit>,
    cells: Vec<u8>,
}

impl Barcode {
    /// A barcode of `units` rows, each `days` cells long from `first`.
    ///
    /// # Errors
    ///
    /// When `cells` isn't `units × days` long.
    pub fn new(first: NaiveDate, days: u32, units: Vec<Unit>, cells: Vec<u8>) -> Res<Self> {
        let expected = units.len() * usize::try_from(days)?;
        ensure!(cells.len() == expected, "{} cells for {} units × {days} days", cells.len(), units.len());
        Ok(Self { first, days, units, cells })
    }

    #[must_use]
    pub const fn first(&self) -> NaiveDate {
        self.first
    }

    #[must_use]
    pub const fn days(&self) -> u32 {
        self.days
    }

    #[must_use]
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// The date of day `index`.
    #[must_use]
    pub fn date(&self, index: u32) -> NaiveDate {
        self.first.checked_add_days(Days::new(u64::from(index))).unwrap_or(NaiveDate::MAX)
    }

    /// The index of `date`, when the barcode covers it.
    #[must_use]
    pub fn index(&self, date: NaiveDate) -> Option<u32> {
        u32::try_from((date - self.first).num_days()).ok().filter(|index| *index < self.days)
    }

    /// Row `row`'s cells, oldest first.
    #[must_use]
    pub fn row(&self, row: usize) -> &[u8] {
        let width = usize::try_from(self.days).unwrap_or(usize::MAX);
        self.cells.get(row * width..(row + 1) * width).unwrap_or_default()
    }

    /// The cell at `row` on day `index`, `None` where there is no data.
    #[must_use]
    pub fn cell(&self, row: usize, index: u32) -> Option<Cell> {
        self.row(row).get(usize::try_from(index).ok()?).copied().and_then(Cell::unpack)
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16 + self.units.len() * 40 + self.cells.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&self.first.num_days_from_ce().to_le_bytes());
        bytes.extend_from_slice(&self.days.to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(self.units.len()).unwrap_or(u16::MAX).to_le_bytes());
        for unit in &self.units {
            for text in [&unit.docket, &unit.name] {
                let text = &text.as_bytes()[..text.len().min(usize::from(u8::MAX))];
                bytes.push(u8::try_from(text.len()).unwrap_or(u8::MAX));
                bytes.extend_from_slice(text);
            }
        }
        bytes.extend_from_slice(&self.cells);
        bytes
    }

    /// # Errors
    ///
    /// When `bytes` isn't a barcode: the wrong magic, a short header or row list, a name that isn't
    /// UTF-8, or the wrong number of cells.
    pub fn decode(bytes: &[u8]) -> Res<Self> {
        let mut reader = Reader(bytes);
        ensure!(reader.take(4)? == MAGIC, "not a barcode buffer");
        let first = i32::from_le_bytes(reader.array()?);
        let first = NaiveDate::from_num_days_from_ce_opt(first).with_context(|| format!("first day {first} is out of range"))?;
        let days = u32::from_le_bytes(reader.array()?);
        let rows = u16::from_le_bytes(reader.array()?);
        let mut units = Vec::with_capacity(usize::from(rows));
        for _ in 0..rows {
            units.push(Unit {
                docket: reader.text()?,
                name: reader.text()?,
            });
        }
        Self::new(first, days, units, reader.0.to_vec())
    }
}

/// Reads the buffer front to back.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Res<&'a [u8]> {
        if self.0.len() < len {
            bail!("the barcode buffer ends early");
        }
        let (head, tail) = self.0.split_at(len);
        self.0 = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Res<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| anyhow!("the barcode buffer ends early"))
    }

    fn text(&mut self) -> Res<String> {
        let [len] = self.array()?;
        Ok(std::str::from_utf8(self.take(usize::from(len))?).context("a row name isn't UTF-8")?.to_owned())
    }
}

/// Builds a barcode from cells set in any order on or after the first date set.
#[derive(Debug)]
pub struct Packer {
    units: Vec<Unit>,
    rows: Vec<Vec<u8>>,
    first: Option<NaiveDate>,
}

impl Packer {
    #[must_use]
    pub fn new(units: Vec<Unit>) -> Self {
        let rows = vec![Vec::new(); units.len()];
        Self { units, rows, first: None }
    }

    /// Sets `row`'s cell on `date`. The first call's date is the barcode's first day.
    ///
    /// # Errors
    ///
    /// When `row` is out of range or `date` is before the first day.
    pub fn set(&mut self, row: usize, date: NaiveDate, cell: Cell) -> Res<()> {
        let first = *self.first.get_or_insert(date);
        let index = usize::try_from((date - first).num_days()).with_context(|| format!("{date} is before the barcode's first day, {first}"))?;
        let cells = self.rows.get_mut(row).with_context(|| format!("no row {row}"))?;
        if cells.len() <= index {
            cells.resize(index + 1, NO_DATA);
        }
        cells[index] = cell.pack();
        Ok(())
    }

    /// The barcode, every row padded with [`NO_DATA`] to the last day set. `None` when nothing was set.
    ///
    /// # Errors
    ///
    /// When the span is too long for the format.
    pub fn finish(self) -> Res<Option<Barcode>> {
        let Some(first) = self.first else { return Ok(None) };
        let days = self.rows.iter().map(Vec::len).max().unwrap_or_default();
        let mut cells = Vec::with_capacity(days * self.rows.len());
        for mut row in self.rows {
            row.resize(days, NO_DATA);
            cells.append(&mut row);
        }
        Barcode::new(first, u32::try_from(days)?, self.units, cells).map(Some)
    }
}

/// What one pixel shows for a span of one unit's days.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    /// No day in the span has data.
    Empty,
    /// In service, and nothing in `visible` reduced power.
    Full,
    /// `family` lost the most among the visible families, and `depth` is their mean reduction over
    /// the days with data, from 0 to 1.
    Reduced { family: Family, depth: f32 },
    /// Only families outside `visible` reduced power, by `depth`.
    Hidden { depth: f32 },
}

/// The mark for `cells`, counting only `visible` families as reductions to color.
#[must_use]
pub fn mark(cells: &[u8], visible: Families) -> Mark {
    let mut served = 0_u16;
    let mut lost = [0.0_f32; 8];
    let mut hidden = 0.0_f32;
    for cell in cells.iter().copied().filter_map(Cell::unpack) {
        served = served.saturating_add(1);
        if visible.contains(cell.family) {
            lost[usize::from(cell.family.index())] += cell.reduction();
        } else {
            hidden += cell.reduction();
        }
    }
    if served == 0 {
        return Mark::Empty;
    }
    let served = f32::from(served);
    let total: f32 = lost.iter().sum();
    let most = Family::ALL.into_iter().max_by(|a, b| lost[usize::from(a.index())].total_cmp(&lost[usize::from(b.index())]));
    match most {
        Some(family) if total > 0.0 => Mark::Reduced { family, depth: total / served },
        _ if hidden > 0.0 => Mark::Hidden { depth: hidden / served },
        _ => Mark::Full,
    }
}

/// The days pixel column `x` of `width` covers when `days` days span the width: at least one, so a
/// barcode wider than its days repeats each day across several columns.
#[must_use]
pub fn span(x: u32, width: u32, days: u32) -> Range<usize> {
    let width = u64::from(width.max(1));
    let days = u64::from(days);
    let start = u64::from(x) * days / width;
    let end = ((u64::from(x) + 1) * days / width).max(start + 1).min(days.max(start + 1));
    usize::try_from(start).unwrap_or(usize::MAX)..usize::try_from(end).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use anyhow::Context as _;
    use pretty_assertions::assert_eq;

    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap_or(NaiveDate::MIN)
    }

    fn unit(docket: &str, name: &str) -> Unit {
        Unit {
            docket: docket.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn uat_barcode_buffer_every_cell_packs_and_unpacks_losslessly() {
        for family in Family::ALL {
            for bucket in 0..=FULL {
                let cell = Cell { family, bucket };
                assert_eq!(Cell::unpack(cell.pack()), Some(cell), "{family:?} bucket {bucket}");
                assert_ne!(cell.pack(), NO_DATA);
            }
        }
        assert_eq!(Cell::unpack(NO_DATA), None);
    }

    #[test]
    fn uat_barcode_buffer_the_buckets_cover_every_power_once() {
        let mut covered = Vec::new();
        for bucket in 0..=FULL {
            let cell = Cell { family: Family::Forced, bucket };
            for power in cell.power() {
                assert_eq!(super::bucket(power), bucket, "{power}%");
                covered.push(power);
            }
        }
        assert_eq!(covered, (0..=100).collect::<Vec<u8>>());
        assert_eq!(super::bucket(99), FULL - 1, "99% isn't full power");
        assert!(close(Cell::new(Family::Grid, 100).reduction(), 0.0));
        assert!(close(Cell::new(Family::Grid, 0).reduction(), 0.985), "0–3% has its middle at 1.5%");
    }

    #[test]
    fn uat_barcode_buffer_encodes_and_decodes_losslessly() -> Res<()> {
        let mut packer = Packer::new(vec![unit("05000275", "Diablo Canyon 1"), unit("05000323", "Diablo Canyon 2")]);
        packer.set(0, date("2024-01-01"), Cell::new(Family::Refueling, 0))?;
        packer.set(1, date("2024-01-03"), Cell::new(Family::Forced, 55))?;
        packer.set(0, date("2024-01-02"), Cell::new(Family::Refueling, 30))?;
        let barcode = packer.finish()?.context("cells were set")?;

        assert_eq!(barcode.first(), date("2024-01-01"));
        assert_eq!(barcode.days(), 3);
        assert_eq!(barcode.row(0), [Cell::new(Family::Refueling, 0).pack(), Cell::new(Family::Refueling, 30).pack(), NO_DATA]);
        assert_eq!(barcode.row(1), [NO_DATA, NO_DATA, Cell::new(Family::Forced, 55).pack()]);
        assert_eq!(barcode.cell(1, 2), Some(Cell::new(Family::Forced, 55)));
        assert_eq!(barcode.index(date("2024-01-03")), Some(2));
        assert_eq!(barcode.index(date("2024-01-04")), None);
        assert_eq!(barcode.date(2), date("2024-01-03"));

        let decoded = Barcode::decode(&barcode.encode())?;
        assert_eq!(decoded, barcode);
        Ok(())
    }

    #[test]
    fn a_damaged_buffer_fails_to_decode() -> Res<()> {
        let mut packer = Packer::new(vec![unit("05000275", "Diablo Canyon 1")]);
        packer.set(0, date("2024-01-01"), Cell::new(Family::Grid, 90))?;
        let bytes = packer.finish()?.context("cells were set")?.encode();
        assert!(Barcode::decode(&bytes[..bytes.len() - 1]).is_err(), "a missing cell");
        assert!(Barcode::decode(&bytes[..10]).is_err(), "a short header");
        let mut wrong = bytes.clone();
        wrong[0] = b'X';
        assert!(Barcode::decode(&wrong).is_err(), "the wrong magic");
        Ok(())
    }

    #[test]
    fn the_packer_refuses_a_day_before_its_first() -> Res<()> {
        let mut packer = Packer::new(vec![unit("05000275", "Diablo Canyon 1")]);
        packer.set(0, date("2024-01-02"), Cell::new(Family::Grid, 90))?;
        assert!(packer.set(0, date("2024-01-01"), Cell::new(Family::Grid, 90)).is_err());
        assert!(packer.set(1, date("2024-01-03"), Cell::new(Family::Grid, 90)).is_err(), "no row 1");
        assert_eq!(Packer::new(Vec::new()).finish()?, None);
        Ok(())
    }

    #[test]
    fn a_mark_takes_the_visible_family_that_lost_most() {
        let refueling = Cell::new(Family::Refueling, 0).pack();
        let forced = Cell::new(Family::Forced, 50).pack();
        let full = Cell::new(Family::Unattributed, 100).pack();

        assert_eq!(mark(&[NO_DATA, NO_DATA], Families::ALL), Mark::Empty);
        assert_eq!(mark(&[full, NO_DATA], Families::ALL), Mark::Full);
        let Mark::Reduced { family, depth } = mark(&[refueling, forced, full, NO_DATA], Families::ALL) else {
            panic!("a reduced span");
        };
        assert_eq!(family, Family::Refueling);
        // 50% sits in the 50–53% bucket, whose middle is 51.5%.
        assert!(close(depth, (0.985 + 0.485) / 3.0), "{depth}");

        // Filtering refueling out leaves forced to color the span, and hides a refueling-only span.
        let visible = Families::ALL.without(Family::Refueling);
        assert!(matches!(mark(&[refueling, forced], visible), Mark::Reduced { family: Family::Forced, .. }));
        assert!(matches!(mark(&[refueling, full], visible), Mark::Hidden { .. }));
    }

    #[test]
    fn columns_split_the_days_without_gaps() {
        let spans: Vec<_> = (0..4).map(|x| span(x, 4, 10)).collect();
        assert_eq!(spans, [0..2, 2..5, 5..7, 7..10]);
        // Wider than the days: each column still covers a day.
        assert_eq!((0..4).map(|x| span(x, 4, 2)).collect::<Vec<_>>(), [0..1, 0..1, 1..2, 1..2]);
    }

    #[test]
    fn layers_round_trip_their_slugs() {
        for layer in Layer::ALL {
            assert_eq!(Layer::from_slug(layer.slug()), Some(layer));
        }
    }
}
