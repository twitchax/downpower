//! Net capacity, which turns a power reduction into lost energy (DESIGN.md Decision 22): EIA-860's
//! net summer capacity for 1999 to 2006 (`catalog/capacity.csv`, written by `cargo xtask
//! capacity`), and `NUC_STATUS`'s daily rating from 2007 on.

use std::collections::HashMap;

use anyhow::{Context as _, ensure};
use chrono::{Datelike as _, NaiveDate};
use serde::Deserialize;

use crate::Res;
use crate::eia::NucStatus;
use crate::units::Unit;

pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/capacity.csv"));

/// The first year `NUC_STATUS` rates; earlier years come from EIA-860.
pub const NUC_STATUS_FROM: i32 = 2007;

/// The years `catalog/capacity.csv` covers.
const FIRST_YEAR: i32 = 1999;

/// A row of `catalog/capacity.csv`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvRow {
    docket: String,
    year: i32,
    net_summer_mw: f64,
    /// The EIA-860 file the rating came from, which is `year` unless that file lacked the unit.
    eia860_year: i32,
    /// EIA-860's status code, e.g. `OP` or `OS`, for the reviewer.
    #[serde(rename = "status")]
    _status: String,
}

/// Each unit's net summer capacity per year before `NUC_STATUS`, in kW.
#[derive(Debug, Clone, Default)]
pub struct Annual {
    kw: HashMap<(String, i32), u32>,
}

impl Annual {
    /// The committed table.
    ///
    /// # Errors
    ///
    /// When `catalog/capacity.csv` breaks a rule [`Annual::parse`] checks.
    pub fn catalog() -> Res<Self> {
        Self::parse(CATALOG).context("catalog/capacity.csv")
    }

    /// Parses a capacity table.
    ///
    /// # Errors
    ///
    /// When a row doesn't parse, a year falls outside 1999–2006, a rating isn't positive or comes
    /// from an earlier file than its year, or a unit-year appears twice.
    pub fn parse(csv: &str) -> Res<Self> {
        let mut kw = HashMap::new();
        for (index, row) in csv::Reader::from_reader(csv.as_bytes()).deserialize::<CsvRow>().enumerate() {
            let line = index + 2;
            let row = row.with_context(|| format!("line {line}"))?;
            ensure!((FIRST_YEAR..NUC_STATUS_FROM).contains(&row.year), "line {line}: year {} is outside 1999–2006", row.year);
            ensure!(row.eia860_year >= row.year, "line {line}: a {} rating can't come from the {} file", row.year, row.eia860_year);
            let rating = to_kw(row.net_summer_mw).with_context(|| format!("line {line}"))?;
            ensure!(kw.insert((row.docket, row.year), rating).is_none(), "line {line}: the unit-year appears twice");
        }
        Ok(Self { kw })
    }

    /// `docket`'s rating for `year`.
    #[must_use]
    pub fn kw(&self, docket: &str, year: i32) -> Option<u32> {
        self.kw.get(&(docket.to_owned(), year)).copied()
    }
}

/// MW to whole kW: EIA's ratings carry at most three decimals of MW.
fn to_kw(mw: f64) -> Res<u32> {
    ensure!(mw.is_finite() && mw > 0.0 && mw < 10_000.0, "{mw} MW is not a reactor's rating");
    // Bounded just above, so it fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked to be in 0..10,000 MW")]
    Ok((mw * 1000.0).round() as u32)
}

/// `unit`'s net capacity on `date`, in kW: EIA-860 for the year before 2007, `NUC_STATUS`'s latest
/// rating from 2007 on.
#[must_use]
pub fn kw(annual: &Annual, nuc_status: Option<&NucStatus>, unit: &Unit, date: NaiveDate) -> Option<u32> {
    if date.year() < NUC_STATUS_FROM {
        return annual.kw(unit.docket.as_str(), date.year());
    }
    nuc_status?.capacity_mw(&unit.eia, date).and_then(|mw| to_kw(mw).ok())
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    const HEADER: &str = "docket,year,net_summer_mw,eia860_year,status\n";

    #[test]
    fn ratings_are_whole_kw() {
        let annual = Annual::parse(&format!("{HEADER}05000293,1999,664.88,1999,SD\n05000293,2000,664.88,2001,OP\n")).unwrap();

        assert_eq!(annual.kw("05000293", 1999), Some(664_880));
        assert_eq!(annual.kw("05000293", 2001), None);
    }

    #[test]
    fn the_table_is_checked() {
        let cases = [
            ("05000293,2007,664.88,2007,OP\n", "outside 1999–2006"),
            ("05000293,2000,664.88,1999,OP\n", "can't come from the 1999 file"),
            ("05000293,2000,0,2000,OP\n", "not a reactor's rating"),
            ("05000293,2000,664.88,2000,OP\n05000293,2000,664.88,2000,OP\n", "appears twice"),
            ("05000293,2000,664.88,2000\n", "line 2"),
        ];
        for (rows, expected) in cases {
            let error = format!("{:#}", Annual::parse(&format!("{HEADER}{rows}")).unwrap_err());
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }
}
