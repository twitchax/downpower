//! Repo chores that read the raw store, run as `cargo xtask <command>`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, ensure};
use clap::{Parser, Subcommand};

use chrono::{Datelike as _, NaiveDate};

use downpower_ingest::eia::NucStatus;
use downpower_ingest::ledger::{self, Catalog, Unknowns};
use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::status::{self, StatusPage};
use downpower_ingest::tags::{self, Family, Matched, Tags};
use downpower_ingest::units::Units;

mod capacity;
mod crosscheck;

const REASONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/reasons.tsv");
const UNCLASSIFIED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/unclassified.tsv");
const CAPACITY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/capacity.csv");

#[derive(Debug, Parser)]
struct Cli {
    /// The data dir that holds `raw/`.
    #[arg(long, env = "DOWNPOWER_DATA_DIR", default_value = "data")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Rewrites catalog/reasons.tsv: every distinct tag on the stored status pages, with its uses
    /// and its uses on rows below 100% power, most used first.
    Reasons,
    /// Rewrites catalog/unclassified.tsv with every tag in catalog/reasons.tsv that no key or rule
    /// matches. Review the diff: committing it accepts those tags as Unattributed.
    Unclassified,
    /// Prints what the keys and each rule catch in catalog/reasons.tsv, with the most used examples,
    /// for reviewing the tag table.
    Rules {
        /// Examples to print per rule.
        #[arg(long, default_value_t = 8)]
        examples: usize,
    },
    /// Builds the ledger from the raw store, as a rebuild does, and prints the reported power
    /// reduction by family and method. Fails on an unknown unit or tag.
    Summary,
    /// Compares the stored status pages with the newest stored `NUC_STATUS`: how often the two
    /// disagree on a unit-day's power, and the in-service unit-days only EIA reports.
    Eia,
    /// Rewrites catalog/capacity.csv, each unit's net summer capacity for 1999 to 2006, from EIA-860
    /// zips in `dir`: eia860a1999.zip, eia860a2000.zip and eia8602001.zip to eia8602006.zip, from
    /// <https://www.eia.gov/electricity/data/eia860/>.
    Capacity {
        /// The directory holding the zips.
        dir: PathBuf,
    },
    /// Compares the stored 1999–2016 status pages with the CSVs in a clone of
    /// github.com/mike-welch/NRC_Power_Reactor_Status_Reports (DESIGN.md Decision 14), and prints
    /// the agreement rates. The clone stays out of the repo: it has no license.
    Crosscheck {
        /// The clone's `Data` directory.
        dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Reasons => reasons(&cli.data_dir),
        Command::Unclassified => unclassified(),
        Command::Rules { examples } => rules(examples),
        Command::Summary => summary(&cli.data_dir),
        Command::Eia => eia(&cli.data_dir),
        Command::Capacity { dir } => capacity::run(&dir, Path::new(CAPACITY)),
        Command::Crosscheck { dir } => crosscheck::run(&cli.data_dir, &dir),
    }
}

fn reasons(data_dir: &Path) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let mut counts: HashMap<String, (u64, u64)> = HashMap::new();
    for record in store.latest(Series::PowerStatus)?.values() {
        let Some(hash) = &record.sha256 else { continue };
        let page = status::parse(&store.get(hash)?).with_context(|| format!("the status page for {}", record.date))?;
        let StatusPage::Report(rows) = page else { continue };
        for row in rows {
            let Some(reason) = row.detail.and_then(|detail| detail.reason) else { continue };
            for tag in tags::tags(&reason) {
                let (uses, below) = counts.entry(tag).or_default();
                *uses += 1;
                *below += u64::from(row.power < 100);
            }
        }
    }
    let mut rows: Vec<_> = counts.into_iter().collect();
    rows.sort_by(|(a, (a_uses, _)), (b, (b_uses, _))| b_uses.cmp(a_uses).then_with(|| a.cmp(b)));
    let mut text = String::from("tag\tuses\tbelow_100\n");
    for (tag, (uses, below)) in &rows {
        writeln!(text, "{tag}\t{uses}\t{below}")?;
    }
    fs::write(REASONS, text).with_context(|| format!("writing {REASONS}"))?;
    println!("{} distinct tags", rows.len());
    Ok(())
}

fn unclassified() -> anyhow::Result<()> {
    let tags = Tags::catalog()?;
    let reasons = fs::read_to_string(REASONS).with_context(|| format!("reading {REASONS}"))?;
    let mut unmatched: Vec<&str> = reasons.lines().skip(1).filter_map(|line| line.split('\t').next()).filter(|tag| !tags.matches(tag)).collect();
    unmatched.sort_unstable();
    let mut text = String::from("tag\n");
    for tag in &unmatched {
        writeln!(text, "{tag}")?;
    }
    fs::write(UNCLASSIFIED, text).with_context(|| format!("writing {UNCLASSIFIED}"))?;
    println!("{} tags no key or rule matches", unmatched.len());
    Ok(())
}

/// One tag of catalog/reasons.tsv: the tag, its uses, and its uses below 100% power.
fn snapshot() -> anyhow::Result<Vec<(String, u64, u64)>> {
    let text = fs::read_to_string(REASONS).with_context(|| format!("reading {REASONS}"))?;
    text.lines()
        .skip(1)
        .map(|line| {
            let mut fields = line.split('\t');
            let (Some(tag), Some(uses), Some(below)) = (fields.next(), fields.next(), fields.next()) else {
                anyhow::bail!("malformed line {line:?}");
            };
            Ok((tag.to_owned(), uses.parse()?, below.parse()?))
        })
        .collect()
}

/// The tags one key set, rule or list knows.
#[derive(Debug, Default)]
struct Group<'a> {
    name: String,
    uses: u64,
    below: u64,
    sample: Vec<&'a str>,
}

fn rules(examples: usize) -> anyhow::Result<()> {
    let tags = Tags::catalog()?;
    let snapshot = snapshot()?;
    let (total, total_below) = snapshot.iter().fold((0, 0), |(uses, below), row| (uses + row.1, below + row.2));
    // Grouped by how each tag was known: keys, then each rule, then the list, then unknown.
    let mut groups: std::collections::BTreeMap<(u8, usize), Group> = std::collections::BTreeMap::new();
    for (tag, uses, below) in &snapshot {
        let (key, name) = match tags.classify(tag) {
            Some(classified) => match classified.matched {
                Matched::Key => ((0, 0), "keys".to_owned()),
                Matched::Rule(index) => ((1, index), format!("rule {index}: {:?} /{}/", classified.label, tags.rule_pattern(index).unwrap_or_default())),
                Matched::Listed => ((2, 0), "listed in unclassified.tsv".to_owned()),
            },
            None => ((3, 0), "unknown".to_owned()),
        };
        let group = groups.entry(key).or_insert_with(|| Group { name, ..Group::default() });
        group.uses += uses;
        group.below += below;
        if group.sample.len() < examples {
            group.sample.push(tag);
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "a percentage for a human to read")]
    let share = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
    for group in groups.values() {
        println!("{}\n  {:.2}% of uses, {:.2}% of uses below 100%", group.name, share(group.uses, total), share(group.below, total_below));
        for tag in &group.sample {
            println!("    {tag}");
        }
    }
    Ok(())
}

/// Totals for one family or method: unit-days below 100%, percent-points of reduction, lost kWh.
#[derive(Debug, Default, Clone, Copy)]
struct Totals {
    days: u64,
    points: u64,
    kwh: u64,
}

fn summary(data_dir: &Path) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let catalog = Catalog::embedded()?;

    let mut families: BTreeMap<Family, Totals> = BTreeMap::new();
    let mut methods: BTreeMap<&'static str, Totals> = BTreeMap::new();
    let mut causes: HashMap<String, Totals> = HashMap::new();
    let mut unattributed: HashMap<(String, String), u64> = HashMap::new();
    let mut without_capacity: BTreeMap<String, (NaiveDate, NaiveDate, u64)> = BTreeMap::new();
    let started = std::time::Instant::now();
    let built = ledger::build(&store, &catalog, Unknowns::Fail, |days| {
        for day in days.into_iter().filter(|day| day.power < 100) {
            if day.lost_kwh.is_none() {
                let span = without_capacity.entry(day.docket.clone()).or_insert((day.date, day.date, 0));
                span.1 = day.date;
                span.2 += 1;
            }
            let points = u64::from(100 - day.power);
            let kwh = day.lost_kwh.unwrap_or_default();
            let cause = day.cause.clone().unwrap_or_else(|| day.method.as_str().to_owned());
            for slot in [
                families.entry(day.family).or_default(),
                methods.entry(day.method.as_str()).or_default(),
                causes.entry(cause.clone()).or_default(),
            ] {
                slot.days += 1;
                slot.points += points;
                slot.kwh += kwh;
            }
            if day.family == Family::Unattributed {
                *unattributed.entry((day.docket, cause)).or_default() += kwh;
            }
        }
        Ok(())
    })?;
    #[allow(clippy::cast_precision_loss, reason = "shares, unit-years and TWh for a human to read")]
    let line = |name: &str, totals: Totals| {
        println!(
            "  {name:<26} {:>8} unit-days  {:>7.1} unit-years  {:>7.1} TWh  {:>5.1}%",
            totals.days,
            totals.points as f64 / 100.0 / 365.25,
            totals.kwh as f64 / 1e9,
            100.0 * totals.kwh as f64 / built.lost_kwh.max(1) as f64
        );
    };
    println!("built in {:.1?}: {built:?}", started.elapsed());
    println!(
        "{} in-service unit-days; lost output by family (unit-days below 100%, power reduction, energy, share of energy):",
        built.days
    );
    for (family, totals) in &families {
        line(&format!("{family:?}"), *totals);
    }
    println!("by method:");
    for (method, totals) in &methods {
        line(method, *totals);
    }
    let mut causes: Vec<_> = causes.into_iter().collect();
    causes.sort_by_key(|(_, totals)| std::cmp::Reverse(totals.kwh));
    println!("by fine cause, the largest 15:");
    for (cause, totals) in causes.iter().take(15) {
        line(cause, *totals);
    }
    println!("booking no lost output (catalog/tags.toml no_lost_output):");
    for (cause, totals) in causes.iter().filter(|(_, totals)| totals.kwh == 0 && totals.points > 0) {
        line(cause, *totals);
    }
    println!("rows below 100% without a capacity (docket: first, last, rows): {without_capacity:?}");
    let mut largest: Vec<_> = unattributed.into_iter().collect();
    largest.sort_by_key(|(_, kwh)| std::cmp::Reverse(*kwh));
    println!("largest Unattributed shares, by unit and cause or method:");
    for ((docket, why), kwh) in largest.iter().take(12) {
        let name = catalog.units.all().iter().find(|unit| unit.docket.as_str() == docket).map_or("?", |unit| unit.name.as_str());
        #[allow(clippy::cast_precision_loss, reason = "TWh for a human to read")]
        let twh = *kwh as f64 / 1e9;
        println!("  {name:<24} {why:<20} {twh:>6.1} TWh");
    }
    Ok(())
}

/// One unit-day where the NRC page and EIA disagree: the date, the unit, NRC power, EIA power.
type Disagreement<'a> = (NaiveDate, &'a str, u8, u8);

fn eia(data_dir: &Path) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let units = Units::catalog()?;
    let (_, record) = store
        .latest(Series::NucStatus)?
        .into_iter()
        .next_back()
        .context("no NUC_STATUS in the raw store; run `downpower fetch --series nuc-status`")?;
    let nuc_status = NucStatus::parse(&store.get(record.sha256.as_ref().context("a stored NUC_STATUS has a hash")?)?)?;
    nuc_status.check(&units)?;

    let mut reported: HashMap<&str, BTreeSet<NaiveDate>> = HashMap::new();
    let mut compared = 0u64;
    let mut nrc_only: BTreeMap<&str, (NaiveDate, NaiveDate, u64)> = BTreeMap::new();
    let mut by_year: BTreeMap<i32, (u64, u64)> = BTreeMap::new();
    let mut disagreements: Vec<Disagreement<'_>> = Vec::new();
    for (date, record) in store.latest(Series::PowerStatus)? {
        let Some(hash) = &record.sha256 else { continue };
        let StatusPage::Report(rows) = status::parse(&store.get(hash)?).with_context(|| format!("the status page for {date}"))? else {
            continue;
        };
        for row in rows {
            let unit = units.by_nrc_name(&row.unit).with_context(|| format!("{date}: unknown unit {:?}", row.unit))?;
            reported.entry(unit.docket.as_str()).or_default().insert(date);
            if !unit.in_service(date) {
                continue;
            }
            let Some(day) = nuc_status.day(&unit.eia, date) else {
                if date.year() >= 2007 {
                    let span = nrc_only.entry(unit.name.as_str()).or_insert((date, date, 0));
                    span.1 = date;
                    span.2 += 1;
                }
                continue;
            };
            compared += 1;
            let year = by_year.entry(date.year()).or_default();
            year.0 += 1;
            if day.power != row.power {
                year.1 += 1;
                disagreements.push((date, unit.name.as_str(), row.power, day.power));
            }
        }
    }

    #[allow(clippy::cast_precision_loss, reason = "rates for a human to read")]
    let rate = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
    println!("NUC_STATUS fetched {} and ending {:?}", record.date, nuc_status.end());
    println!(
        "{compared} in-service unit-days both report; they disagree on {} ({:.3}%)",
        disagreements.len(),
        rate(disagreements.len() as u64, compared)
    );
    println!(
        "{} in-service unit-days from 2007 on that only the NRC pages report (unit: first, last, days):",
        nrc_only.values().map(|(.., days)| days).sum::<u64>()
    );
    for (unit, (first, last, days)) in &nrc_only {
        println!("  {unit:<24} {first} {last} {days:>5}");
    }
    println!("disagreements by year:");
    for (year, (both, differ)) in by_year.iter().filter(|(_, (_, differ))| *differ > 0) {
        println!("  {year}  {differ:>5} of {both:>6} ({:.3}%)", rate(*differ, *both));
    }
    let mut buckets: BTreeMap<&str, u64> = BTreeMap::new();
    for &(_, _, nrc, eia) in &disagreements {
        let bucket = match nrc.abs_diff(eia) {
            1 => "1 point",
            2..=5 => "2-5 points",
            6..=20 => "6-20 points",
            _ => "over 20 points",
        };
        *buckets.entry(bucket).or_default() += 1;
    }
    println!("by size: {buckets:?}");
    let mut by_unit: HashMap<&str, u64> = HashMap::new();
    for &(_, unit, ..) in &disagreements {
        *by_unit.entry(unit).or_default() += 1;
    }
    let mut by_unit: Vec<_> = by_unit.into_iter().collect();
    by_unit.sort_by_key(|(unit, count)| (std::cmp::Reverse(*count), *unit));
    println!("most disagreements by unit: {:?}", &by_unit[..by_unit.len().min(10)]);
    println!("examples (date, unit, NRC, EIA):");
    for disagreement in disagreements.iter().step_by((disagreements.len() / 20).max(1)) {
        println!("  {disagreement:?}");
    }

    let mut gaps: BTreeMap<NaiveDate, Vec<&str>> = BTreeMap::new();
    for unit in units.all() {
        let dates = reported.get(unit.docket.as_str());
        for (date, _) in nuc_status.gaps(unit, |date| dates.is_some_and(|dates| dates.contains(&date))) {
            gaps.entry(date).or_default().push(unit.name.as_str());
        }
    }
    println!("{} in-service unit-days only EIA reports, on {} dates:", gaps.values().map(Vec::len).sum::<usize>(), gaps.len());
    for (date, names) in &gaps {
        println!("  {date}  {:>3} units  e.g. {:?}", names.len(), &names[..names.len().min(3)]);
    }
    Ok(())
}
