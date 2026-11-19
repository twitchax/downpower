//! When each state legalized cannabis (DESIGN.md Decision 37, PRD-0005 T-004): `catalog/cannabis.csv`,
//! hand-curated from Wikipedia's timeline and checked against NCSL and the statutes, one row per
//! state and DC with its adult-use, licensed-sales and comprehensive-medical effective dates. The
//! file opens with `#` lines carrying its attribution notice (CC BY-SA).

use std::collections::BTreeMap;

use anyhow::{Context as _, ensure};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::Res;

pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/cannabis.csv"));

/// One state's dates; `None` where it hasn't happened.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Law {
    pub state: String,
    /// Adult (non-medical) possession legal under state law.
    pub adult_use: Option<NaiveDate>,
    /// The first licensed adult-use retail sales.
    pub sales: Option<NaiveDate>,
    /// A comprehensive medical program in effect, as NCSL defines one.
    pub medical: Option<NaiveDate>,
    /// Space-separated URLs.
    pub source: String,
}

impl Law {
    /// Whether adult use was legal on `date`.
    #[must_use]
    pub fn adult_use_on(&self, date: NaiveDate) -> bool {
        self.adult_use.is_some_and(|since| since <= date)
    }

    /// Whether a comprehensive medical program was in effect on `date`.
    #[must_use]
    pub fn medical_on(&self, date: NaiveDate) -> bool {
        self.medical.is_some_and(|since| since <= date)
    }
}

/// Every state's law, by USPS code.
#[derive(Debug, Clone, Default)]
pub struct Laws(BTreeMap<String, Law>);

impl Laws {
    /// The committed catalog.
    ///
    /// # Errors
    ///
    /// When `catalog/cannabis.csv` breaks a rule [`Laws::parse`] checks.
    pub fn catalog() -> Res<Self> {
        Self::parse(CATALOG).context("catalog/cannabis.csv")
    }

    /// Parses a legalization table, skipping its `#` lines.
    ///
    /// # Errors
    ///
    /// When a row doesn't parse, a state isn't two capital letters or appears twice, a row cites no
    /// source, or sales come without adult use or before it.
    pub fn parse(csv: &str) -> Res<Self> {
        let mut laws = BTreeMap::new();
        let mut reader = csv::ReaderBuilder::new().comment(Some(b'#')).from_reader(csv.as_bytes());
        for (index, law) in reader.deserialize::<Law>().enumerate() {
            let row = index + 1;
            let law = law.with_context(|| format!("row {row}"))?;
            ensure!(law.state.len() == 2 && law.state.bytes().all(|byte| byte.is_ascii_uppercase()), "row {row}: state {:?}", law.state);
            ensure!(
                law.source.split(' ').all(|url| url.starts_with("https://")),
                "row {row}: {} cites no source, or one that isn't a URL",
                law.state
            );
            if let Some(sales) = law.sales {
                ensure!(law.adult_use.is_some_and(|adult_use| adult_use <= sales), "row {row}: {} sells before adult use is legal", law.state);
            }
            let state = law.state.clone();
            ensure!(laws.insert(state.clone(), law).is_none(), "row {row}: {state} appears twice");
        }
        Ok(Self(laws))
    }

    /// The state's law, if the table has the state.
    #[must_use]
    pub fn get(&self, state: &str) -> Option<&Law> {
        self.0.get(state)
    }

    /// Every state's law, by code.
    pub fn iter(&self) -> impl Iterator<Item = &Law> {
        self.0.values()
    }
}
