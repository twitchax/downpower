//! Repo chores that read the raw store, run as `cargo xtask <command>`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, ensure};
use clap::{Parser, Subcommand};

use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::status::{self, StatusPage};
use downpower_ingest::tags::{self, Matched, Tags};

const REASONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/reasons.tsv");
const UNCLASSIFIED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/unclassified.tsv");

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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Reasons => reasons(&cli.data_dir),
        Command::Unclassified => unclassified(),
        Command::Rules { examples } => rules(examples),
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
