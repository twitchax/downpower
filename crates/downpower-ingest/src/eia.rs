//! EIA's `NUC_STATUS` bulk file (DESIGN.md Decision 11): the NRC power column as EIA republishes
//! it, daily per generator from 2007, with each generator's net capacity. It fills the unit-days
//! the NRC pages miss and supplies the capacity that turns a power reduction into energy. Where an NRC
//! page and EIA both report a unit-day, the NRC page wins.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read as _};

use anyhow::{Context as _, bail, ensure};
use chrono::{NaiveDate, Utc};
use reqwest::StatusCode;
use serde::Deserialize;

use crate::raw::{RawStore, Record, Series, Source};
use crate::units::{EiaGenerator, Unit, Units};
use crate::{Res, Void};

/// The keyless bulk file: about 11 MB zipped around 58 MB of NDJSON.
pub const NUC_STATUS_URL: &str = "https://www.eia.gov/opendata/bulk/NUC_STATUS.zip";

/// The most NDJSON a download may unzip to, against a corrupt or hostile size field.
const MAX_LEN: u64 = 512 << 20;

/// Fetches `NUC_STATUS.zip` from `url` and stores the NDJSON file inside it, which is what the
/// record's hash names. The record's date is the fetch date, since the file covers every day.
///
/// # Errors
///
/// When the request fails, the response is not a 200, the body is not a zip holding exactly one
/// file, or the raw store cannot be written.
pub async fn fetch(client: &reqwest::Client, url: &str, store: &RawStore) -> Res<Record> {
    let fetched_at = Utc::now();
    let response = client.get(url).send().await.with_context(|| format!("fetching {url}"))?;
    let status = response.status();
    ensure!(status == StatusCode::OK, "{url} returned {status}");
    let body = response.bytes().await.with_context(|| format!("reading {url}"))?;
    let ndjson = unzip(&body).with_context(|| format!("unzipping {url}"))?;
    let record = Record {
        source: Source::Eia,
        series: Series::NucStatus,
        date: fetched_at.date_naive(),
        url: url.to_owned(),
        status: status.as_u16(),
        captured_at: fetched_at,
        fetched_at,
        sha256: Some(store.put(&ndjson)?),
        len: u64::try_from(ndjson.len()).ok(),
    };
    store.append(&record)?;
    tracing::info!(%url, len = ndjson.len(), "stored NUC_STATUS");
    Ok(record)
}

/// The one file inside a bulk zip. Reading to the end checks its CRC.
fn unzip(body: &[u8]) -> Res<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(body)).context("not a zip")?;
    ensure!(archive.len() == 1, "expected one file in the zip, found {}", archive.len());
    let file = archive.by_index(0).context("opening the zipped file")?;
    ensure!(file.size() <= MAX_LEN, "the zipped file claims {} bytes", file.size());
    let mut bytes = Vec::with_capacity(usize::try_from(file.size()).context("the zipped file's size")?);
    file.take(MAX_LEN + 1).read_to_end(&mut bytes).context("inflating the zipped file")?;
    ensure!(bytes.len() as u64 <= MAX_LEN, "the zipped file inflates past {MAX_LEN} bytes");
    Ok(bytes)
}

/// One generator-day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Day {
    /// Net capacity in MW.
    pub capacity_mw: f64,
    /// The percent of capacity in service: 100 minus EIA's percent outage, which is always whole.
    pub power: u8,
}

/// The generator-level series of one `NUC_STATUS` file. The plant and US totals are skipped, and so
/// is MW out, which is capacity times percent outage on every day of the file.
#[derive(Debug, Clone, Default)]
pub struct NucStatus {
    generators: HashMap<EiaGenerator, BTreeMap<NaiveDate, Day>>,
}

/// One NDJSON line. Category lines carry no `series_id`.
#[derive(Deserialize)]
struct Line<'a> {
    #[serde(borrow)]
    series_id: Option<&'a str>,
    #[serde(borrow)]
    data: Option<Vec<(&'a str, f64)>>,
}

/// What a series measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Capacity,
    Outage,
    OutagePct,
}

/// Splits `NUC_STATUS.{CAP|OUT|OUT_PCT}.{plant}-{generator}.D`. A plant or US total (no generator)
/// is `None`.
fn series_id(id: &str) -> Res<Option<(Kind, EiaGenerator)>> {
    let inner = id.strip_prefix("NUC_STATUS.").and_then(|rest| rest.strip_suffix(".D")).context("not a daily NUC_STATUS series")?;
    let (kind, key) = inner.split_once('.').context("no measure")?;
    let kind = match kind {
        "CAP" => Kind::Capacity,
        "OUT" => Kind::Outage,
        "OUT_PCT" => Kind::OutagePct,
        other => bail!("unknown measure {other:?}"),
    };
    let Some((plant, generator)) = key.split_once('-') else { return Ok(None) };
    ensure!(!generator.is_empty(), "no generator");
    let plant = plant.parse().with_context(|| format!("plant {plant:?}"))?;
    Ok(Some((kind, EiaGenerator { plant, generator: generator.to_owned() })))
}

/// A series' points by date, each value checked by `value`.
fn points<T>(data: &[(&str, f64)], value: impl Fn(f64) -> Res<T>) -> Res<BTreeMap<NaiveDate, T>> {
    let mut points = BTreeMap::new();
    for &(date, raw) in data {
        let day = NaiveDate::parse_from_str(date, "%Y%m%d").with_context(|| format!("date {date:?}"))?;
        let parsed = value(raw).with_context(|| format!("{date}: {raw}"))?;
        ensure!(points.insert(day, parsed).is_none(), "{date} appears twice");
    }
    Ok(points)
}

impl NucStatus {
    /// Parses a `NUC_STATUS.txt`.
    ///
    /// # Errors
    ///
    /// When a line is not JSON, a series id or date does not parse, a series appears twice, a
    /// capacity is negative, a percent outage is not a whole number from 0 to 100, or a generator's
    /// capacity and percent outage do not cover the same days.
    pub fn parse(ndjson: &[u8]) -> Res<Self> {
        let mut capacity: HashMap<EiaGenerator, BTreeMap<NaiveDate, f64>> = HashMap::new();
        let mut power: HashMap<EiaGenerator, BTreeMap<NaiveDate, u8>> = HashMap::new();
        for (index, line) in ndjson.split(|&byte| byte == b'\n').enumerate() {
            if line.trim_ascii().is_empty() {
                continue;
            }
            let line: Line<'_> = serde_json::from_slice(line).with_context(|| format!("line {}", index + 1))?;
            let Some(id) = line.series_id else { continue };
            let Some((kind, generator)) = series_id(id).with_context(|| format!("series {id:?}"))? else {
                continue;
            };
            let data = line.data.with_context(|| format!("{id} has no data"))?;
            let fresh = match kind {
                Kind::Outage => continue,
                Kind::Capacity => capacity
                    .insert(
                        generator,
                        points(&data, |mw| {
                            ensure!(mw.is_finite() && mw >= 0.0, "not a capacity");
                            Ok(mw)
                        })
                        .with_context(|| id.to_owned())?,
                    )
                    .is_none(),
                Kind::OutagePct => power
                    .insert(
                        generator,
                        points(&data, |pct| {
                            ensure!(pct.fract() == 0.0 && (0.0..=100.0).contains(&pct), "not a whole percent");
                            // A whole number from 0 to 100 converts exactly.
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked just above")]
                            Ok(100 - pct as u8)
                        })
                        .with_context(|| id.to_owned())?,
                    )
                    .is_none(),
            };
            ensure!(fresh, "{id} appears twice");
        }

        let mut generators = HashMap::with_capacity(power.len());
        for (generator, power) in power {
            let capacity = capacity.remove(&generator).with_context(|| format!("{generator:?} has a percent outage but no capacity"))?;
            ensure!(capacity.keys().eq(power.keys()), "{generator:?}: capacity and percent outage cover different days");
            let days = capacity
                .into_iter()
                .zip(power.into_values())
                .map(|((date, capacity_mw), power)| (date, Day { capacity_mw, power }))
                .collect();
            generators.insert(generator, days);
        }
        if let Some(generator) = capacity.keys().next() {
            bail!("{generator:?} has a capacity but no percent outage");
        }
        Ok(Self { generators })
    }

    /// Every generator in the file.
    pub fn generators(&self) -> impl Iterator<Item = &EiaGenerator> {
        self.generators.keys()
    }

    /// The last day any generator reports.
    #[must_use]
    pub fn end(&self) -> Option<NaiveDate> {
        self.generators.values().filter_map(|days| days.keys().next_back()).max().copied()
    }

    /// What `generator` reported on `date`.
    #[must_use]
    pub fn day(&self, generator: &EiaGenerator, date: NaiveDate) -> Option<Day> {
        self.generators.get(generator)?.get(&date).copied()
    }

    /// `generator`'s net capacity on `date`: the latest reported on or before it, which carries a
    /// rating across the days EIA skips and past the end of the file.
    #[must_use]
    pub fn capacity_mw(&self, generator: &EiaGenerator, date: NaiveDate) -> Option<f64> {
        self.generators.get(generator)?.range(..=date).next_back().map(|(_, day)| day.capacity_mw)
    }

    /// Checks that every generator maps to a unit.
    ///
    /// # Errors
    ///
    /// Listing the generators `units` doesn't know (uat-006).
    pub fn check(&self, units: &Units) -> Void {
        let mut unknown: Vec<_> = self.generators().filter(|generator| units.by_eia(generator).is_none()).collect();
        unknown.sort_by(|a, b| (a.plant, &a.generator).cmp(&(b.plant, &b.generator)));
        ensure!(unknown.is_empty(), "NUC_STATUS generators missing from catalog/units.csv: {unknown:?}");
        Ok(())
    }

    /// The days EIA fills for `unit`: each day its generator reports while the unit was in service
    /// that `reported` says the NRC pages lack, oldest first.
    pub fn gaps<'a>(&'a self, unit: &'a Unit, reported: impl Fn(NaiveDate) -> bool + 'a) -> impl Iterator<Item = (NaiveDate, Day)> + 'a {
        self.generators
            .get(&unit.eia)
            .into_iter()
            .flatten()
            .filter(move |(date, _)| unit.in_service(**date) && !reported(**date))
            .map(|(date, day)| (*date, *day))
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn series_ids_split_into_measure_and_generator() {
        let (kind, generator) = series_id("NUC_STATUS.OUT_PCT.6099-2.D").unwrap().unwrap();

        assert_eq!(kind, Kind::OutagePct);
        assert_eq!(generator, EiaGenerator { plant: 6099, generator: "2".to_owned() });
    }

    #[test]
    fn plant_and_us_totals_are_skipped() {
        assert!(series_id("NUC_STATUS.CAP.6099.D").unwrap().is_none());
        assert!(series_id("NUC_STATUS.OUT.US.D").unwrap().is_none());
    }

    #[test]
    fn a_series_id_outside_the_scheme_fails() {
        assert!(series_id("NUC_STATUS.GEN.6099-2.D").is_err());
        assert!(series_id("NUC_STATUS.CAP.6099-2.M").is_err());
        assert!(series_id("ELEC.CAP.6099-2.D").is_err());
        assert!(series_id("NUC_STATUS.CAP.abc-2.D").is_err());
        assert!(series_id("NUC_STATUS.CAP.6099-.D").is_err());
    }
}
