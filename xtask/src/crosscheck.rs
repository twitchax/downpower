//! `cargo xtask crosscheck`: our parse of the 1999–2016 status pages against the CSVs a third party
//! scraped from the old reading-room pages (DESIGN.md Decision 14). They're a checker, never a
//! source, and their repo has no license, so they stay in a scratch clone.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, ensure};
use chrono::{Datelike as _, NaiveDate};

use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::status::{self, StatusPage};
use downpower_ingest::units::Units;

/// The old reading-room pages' names for units the live pages name otherwise, lowercased.
const OLD_NAMES: [(&str, &str); 21] = [
    ("callaway 1", "callaway"),
    ("clinton 1", "clinton"),
    ("columbia generating", "columbia generating station"),
    ("washington nuclear 2", "columbia generating station"),
    ("wnp-2", "columbia generating station"),
    ("cook 1", "d.c. cook 1"),
    ("cook 2", "d.c. cook 2"),
    ("cooper 1", "cooper"),
    ("davis besse 1", "davis-besse"),
    ("duane arnold 1", "duane arnold"),
    ("fitzpatrick 1", "fitzpatrick"),
    ("ft calhoun 1", "fort calhoun"),
    ("ginna 1", "ginna"),
    ("kewaunee 1", "kewaunee"),
    ("monticello 1", "monticello"),
    ("oyster creek 1", "oyster creek"),
    ("palisades 1", "palisades"),
    ("summer 1", "summer"),
    ("vermont yankee 1", "vermont yankee"),
    ("beaver valley-1", "beaver valley 1"),
    ("beaver valley-2", "beaver valley 2"),
];

/// One of their rows: the power, and the reason when the line splits cleanly enough to recover it.
struct Theirs {
    power: Option<u8>,
    reason: Option<String>,
}

/// Lowercase with whitespace collapsed, so case, line breaks and padding don't count as
/// disagreement.
fn normal(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Every unit name, ours or theirs, lowercased, to its docket.
fn dockets(units: &Units) -> HashMap<String, String> {
    let mut dockets: HashMap<String, String> = units
        .all()
        .iter()
        .flat_map(|unit| unit.nrc_names.iter().map(|name| (normal(name), unit.docket.as_str().to_owned())))
        .collect();
    for (old, new) in OLD_NAMES {
        if let Some(docket) = dockets.get(new).cloned() {
            dockets.insert(old.to_owned(), docket);
        }
    }
    dockets
}

/// Their CSVs don't quote, so a reason with commas spills into the next columns. The date, unit and
/// power always lead, and a row with detail ends in the change mark and the scram count, so the
/// reason is whatever lies between `Down` and those two. 2002 writes power as `100.00000`.
fn parse_line(line: &str) -> Option<(NaiveDate, String, Theirs)> {
    let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split(',').collect();
    let date = fields.first()?.trim().parse().ok()?;
    let unit = normal(fields.get(1)?);
    let power = fields.get(2)?.trim().parse::<f64>().ok().filter(|power| power.fract() == 0.0 && (0.0..=100.0).contains(power));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a whole number from 0 to 100")]
    let power = power.map(|power| power as u8);
    let reason = (fields.len() >= 6).then(|| fields[4..fields.len() - 2].join(","));
    Some((date, unit, Theirs { power, reason }))
}

fn csvs(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(csvs(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "csv") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Their rows by date and docket, the names no docket matched, and the lines that didn't parse.
type Loaded = (HashMap<(NaiveDate, String), Theirs>, BTreeMap<String, u64>, u64);

fn load(dir: &Path, dockets: &HashMap<String, String>) -> anyhow::Result<Loaded> {
    let mut theirs = HashMap::new();
    let mut unnamed: BTreeMap<String, u64> = BTreeMap::new();
    let mut unparsed = 0;
    for file in csvs(dir)? {
        let text = fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
        for line in String::from_utf8_lossy(&text).lines().skip(1).filter(|line| !line.trim().is_empty()) {
            let Some((date, unit, row)) = parse_line(line) else {
                unparsed += 1;
                continue;
            };
            match dockets.get(&unit) {
                Some(docket) => {
                    theirs.insert((date, docket.clone()), row);
                }
                None => *unnamed.entry(unit).or_default() += 1,
            }
        }
    }
    Ok((theirs, unnamed, unparsed))
}

/// Per year: rows in both, power agreements, rows whose reason both have, how many of those agree
/// outright and how many only because theirs is cut short, and rows only theirs or only ours.
#[derive(Debug, Default, Clone, Copy)]
struct Year {
    both: u64,
    power_agrees: u64,
    reasons: u64,
    reason_agrees: u64,
    /// Theirs is a prefix of ours: their early rows cut reasons at about 75 characters.
    reason_truncated: u64,
    only_theirs: u64,
    only_ours: u64,
}

impl Year {
    fn add(&mut self, other: &Self) {
        self.both += other.both;
        self.power_agrees += other.power_agrees;
        self.reasons += other.reasons;
        self.reason_agrees += other.reason_agrees;
        self.reason_truncated += other.reason_truncated;
        self.only_theirs += other.only_theirs;
        self.only_ours += other.only_ours;
    }

    fn print(&self, label: &str) {
        #[allow(clippy::cast_precision_loss, reason = "rates for a human to read")]
        let rate = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
        println!(
            "{label:<5} {:>7}  {:>11.3}%  {:>7}  {:>17.3}%  {:>11}  {:>9}",
            self.both,
            rate(self.power_agrees, self.both),
            self.reasons,
            rate(self.reason_agrees + self.reason_truncated, self.reasons),
            self.only_theirs,
            self.only_ours
        );
    }
}

pub fn run(data_dir: &Path, dir: &Path) -> anyhow::Result<()> {
    let units = Units::catalog()?;
    let dockets = dockets(&units);
    let (mut theirs, unnamed, unparsed) = load(dir, &dockets)?;
    ensure!(!theirs.is_empty(), "no CSV rows under {}", dir.display());
    let last = theirs.keys().map(|(date, _)| *date).max().context("a date")?;

    let store = RawStore::open(data_dir)?;
    let mut years: BTreeMap<i32, Year> = BTreeMap::new();
    let mut examples: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (date, record) in store.latest(Series::PowerStatus)?.range(..=last) {
        let Some(hash) = &record.sha256 else { continue };
        let StatusPage::Report(rows) = status::parse(&store.get(hash)?).with_context(|| format!("the status page for {date}"))? else {
            continue;
        };
        let year = years.entry(date.year()).or_default();
        for row in rows {
            let docket = units.by_nrc_name(&row.unit).with_context(|| format!("{date}: unknown unit {:?}", row.unit))?.docket.as_str();
            let Some(their) = theirs.remove(&(*date, docket.to_owned())) else {
                year.only_ours += 1;
                continue;
            };
            year.both += 1;
            let mut example = |kind: &'static str, text: String| {
                let kept = examples.entry(kind).or_default();
                if kept.len() < 6 {
                    kept.push(text);
                }
            };
            if their.power == Some(row.power) {
                year.power_agrees += 1;
            } else {
                example("power", format!("{date} {}: ours {} theirs {:?}", row.unit, row.power, their.power));
            }
            let ours = row.detail.and_then(|detail| detail.reason).unwrap_or_default();
            let Some(reason) = their.reason else { continue };
            year.reasons += 1;
            let (theirs_normal, ours_normal) = (normal(&reason), normal(&ours));
            if theirs_normal == ours_normal {
                year.reason_agrees += 1;
            } else if !theirs_normal.is_empty() && ours_normal.starts_with(&theirs_normal) {
                year.reason_truncated += 1;
            } else {
                example("reason", format!("{date} {}: ours {ours:?} theirs {reason:?}", row.unit));
            }
        }
    }
    for (date, _) in theirs.keys() {
        years.entry(date.year()).or_default().only_theirs += 1;
    }

    println!("year     both  power agrees  reasons  agree or truncated  only theirs  only ours");
    let mut total = Year::default();
    for (year, counts) in &years {
        counts.print(&year.to_string());
        total.add(counts);
    }
    total.print("all");
    println!("{unparsed} lines unparsed, {} reasons truncated, unit names no docket matched: {unnamed:?}", total.reason_truncated);
    for (kind, kept) in &examples {
        for text in kept {
            println!("  {kind}: {text}");
        }
    }
    Ok(())
}
