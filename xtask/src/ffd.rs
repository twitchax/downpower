//! `cargo xtask ffd`: identifies every fitness-for-duty event in the raw store (PRD-0005 T-001) and
//! rewrites `catalog/ffd.tsv`, one line per event, reviewed like the reasons snapshot. With
//! `--candidates` it also prints the events no section or list makes FFD whose text says fitness for
//! duty, for the include list's review; with `--texts <file>` it writes each FFD event's title and
//! text there, for the phrase rules' review.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, ensure};
use chrono::Datelike as _;

use downpower_ingest::events::{Event, EventSet};
use downpower_ingest::facilities::Identity;
use downpower_ingest::ffd::{Ffd, Tags, Why};
use downpower_ingest::ledger::{Catalog, Unknowns};
use downpower_ingest::raw::RawStore;
use downpower_ingest::units::Docket;

const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/ffd.tsv");

/// Where an event happened, as the snapshot writes it.
fn place(identity: Option<&Identity>) -> String {
    match identity {
        Some(Identity::Reactor { dockets }) => dockets.iter().map(Docket::as_str).collect::<Vec<_>>().join(","),
        Some(Identity::Facility { docket, .. }) => docket.as_str().to_owned(),
        Some(Identity::Licensee { state, license, .. }) => format!("{state}:{license}"),
        None => "-".to_owned(),
    }
}

/// A cell with its tabs and line breaks made spaces.
fn cell(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Each tag as the catalog spells it: substance, role, kind and test.
fn slugs(tags: Tags) -> anyhow::Result<[String; 4]> {
    let value = serde_json::to_value(tags)?;
    Ok(["substance", "role", "kind", "test"].map(|dimension| value[dimension].as_str().unwrap_or_default().to_owned()))
}

fn codes(event: &Event) -> String {
    event.sections.iter().filter_map(|section| section.code.as_deref()).map(cell).collect::<Vec<_>>().join("; ")
}

pub fn run(data_dir: &Path, candidates: bool, texts: Option<&Path>) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let catalog = Catalog::embedded()?;
    let set = EventSet::build(&store, &catalog.facilities, Unknowns::Fail)?;
    let ffd = Ffd::catalog()?;

    let mut snapshot = String::from("number\tdate\tcategory\tstate\twhere\twhy\tsubstance\trole\tkind\ttest\ttitle\n");
    let mut by_tag: BTreeMap<String, usize> = BTreeMap::new();
    let mut dump = String::new();
    let mut by_year: BTreeMap<i32, usize> = BTreeMap::new();
    let mut by_why: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_category: BTreeMap<&str, usize> = BTreeMap::new();
    for (number, event) in &set.events {
        let Some(why) = ffd.identify(event) else { continue };
        let why = match why {
            Why::Section(section) => section,
            Why::Listed => "listed".to_owned(),
        };
        *by_year.entry(event.occurred.date.year()).or_default() += 1;
        *by_why.entry(why.clone()).or_default() += 1;
        *by_category.entry(event.category.label()).or_default() += 1;
        let tags = ffd.tags(event);
        let names = slugs(tags)?;
        for (dimension, name) in ["substance", "role", "kind", "test"].iter().zip(&names) {
            *by_tag.entry(format!("{dimension}={name}")).or_default() += 1;
        }
        writeln!(
            snapshot,
            "{number}\t{}\t{}\t{}\t{}\t{why}\t{}\t{}",
            event.occurred.date,
            event.category.label(),
            event.state.as_deref().unwrap_or("-"),
            place(set.identities.get(number)),
            names.join("\t"),
            cell(event.title.as_deref().unwrap_or("")),
        )?;
        writeln!(
            dump,
            "EN {number} {} [{}] {}\n{}\n{}\n",
            event.occurred.date,
            codes(event),
            event.state.as_deref().unwrap_or("-"),
            event.title.as_deref().unwrap_or(""),
            event.text
        )?;
    }
    fs::write(SNAPSHOT, &snapshot).with_context(|| format!("writing {SNAPSHOT}"))?;
    let total: usize = by_year.values().sum();
    println!("{total} FFD events");
    println!("by why: {by_why:?}");
    println!("by category: {by_category:?}");
    println!("by year: {by_year:?}");
    println!("by tag: {by_tag:#?}");

    if candidates {
        println!("\nevents no section or list makes FFD whose text says fitness for duty:");
        for (number, event) in &set.events {
            if ffd.identify(event).is_some() {
                continue;
            }
            let text = format!("{} {}", event.title.as_deref().unwrap_or(""), event.text).to_lowercase();
            if text.contains("fitness for duty") || text.contains("fitness-for-duty") || text.contains(" ffd") {
                writeln!(
                    dump,
                    "CANDIDATE EN {number} {} [{}]\n{}\n{}\n",
                    event.occurred.date,
                    codes(event),
                    event.title.as_deref().unwrap_or(""),
                    event.text
                )?;
                println!(
                    "  EN {number} {} {}{} [{}] {}",
                    event.occurred.date,
                    event.category.label(),
                    if event.retracted { " (retracted)" } else { "" },
                    codes(event),
                    cell(event.title.as_deref().unwrap_or(""))
                );
            }
        }
    }
    if let Some(texts) = texts {
        fs::write(texts, &dump).with_context(|| format!("writing {}", texts.display()))?;
    }
    Ok(())
}
