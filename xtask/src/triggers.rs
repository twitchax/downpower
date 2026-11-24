//! `cargo xtask triggers`: tags every scram Event Notification in the raw store (PRD-0007 T-002)
//! and rewrites `catalog/triggers.tsv`, one line per event, reviewed like the reasons snapshot. With
//! `--episodes` it also builds the ledger and every scram's episode (T-003), and prints how the
//! episodes' energy compares with what the ledger attributes to the `scram` cause.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, ensure};
use chrono::Datelike as _;

use downpower_ingest::episodes::Collector;
use downpower_ingest::episodes::End;
use downpower_ingest::events::{Event, EventSet, Scram};
use downpower_ingest::facilities::Identity;
use downpower_ingest::ledger::{self, Catalog, Unknowns};
use downpower_ingest::raw::RawStore;
use downpower_ingest::triggers::{Circumstance, Decided, System, Triggers};
use downpower_ingest::units::Docket;

const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../catalog/triggers.tsv");

/// The fine cause the ledger books a scram's days to.
const SCRAM: &str = "scram";

/// What decided a tag, as the snapshot writes it.
fn decided(decided: Decided) -> String {
    match decided {
        Decided::Rule(index) => format!("rule {index}"),
        Decided::Signal(index) => format!("signal {index}"),
        Decided::Assigned => "assigned".to_owned(),
        Decided::Nothing => "none".to_owned(),
    }
}

/// Each scrammed unit's SCRAM Code, as the page writes it.
fn codes(event: &Event) -> String {
    let code = |scram: Scram| match scram {
        Scram::AutoReactor => "A/R",
        Scram::ManualReactor => "M/R",
        Scram::Auto => "A",
        Scram::Manual => "M",
        Scram::No => "N",
    };
    event
        .units
        .iter()
        .filter_map(|unit| unit.scram)
        .filter(|scram| *scram != Scram::No)
        .map(code)
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether `event` is a scram the tags cover: unretracted, with a unit given a SCRAM Code other than `N`.
fn scrammed(event: &Event) -> bool {
    !event.retracted && event.units.iter().any(|unit| unit.scram.is_some_and(|scram| scram != Scram::No))
}

pub fn run(data_dir: &Path, episodes: bool) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let catalog = Catalog::embedded()?;
    let set = EventSet::build(&store, &catalog.facilities, Unknowns::Fail)?;
    let triggers = Triggers::catalog()?;
    triggers.check(&set)?;

    let mut snapshot = String::from("number\tdate\tcodes\twhere\tsystem\tcircumstance\tsystem by\tcircumstance by\ttitle\n");
    let mut systems: BTreeMap<(i32, System), usize> = BTreeMap::new();
    let mut circumstances: BTreeMap<Circumstance, usize> = BTreeMap::new();
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for (number, event) in set.events.iter().filter(|(_, event)| scrammed(event)) {
        let (tags, [by_system, by_circumstance]) = triggers.explain(event);
        let place = match set.identities.get(number) {
            Some(Identity::Reactor { dockets }) => dockets.iter().map(Docket::as_str).collect::<Vec<_>>().join(","),
            _ => "-".to_owned(),
        };
        let era = event.occurred.date.year() / 5 * 5;
        *systems.entry((era, tags.system)).or_default() += 1;
        *circumstances.entry(tags.circumstance).or_default() += 1;
        *by.entry(decided(by_system)).or_default() += 1;
        writeln!(
            snapshot,
            "{number}\t{}\t{}\t{place}\t{}\t{}\t{}\t{}\t{}",
            event.occurred.date,
            codes(event),
            tags.system.slug(),
            tags.circumstance.slug(),
            decided(by_system),
            decided(by_circumstance),
            event.title.as_deref().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
        )?;
    }
    fs::write(SNAPSHOT, &snapshot).with_context(|| format!("writing {SNAPSHOT}"))?;
    let total: usize = circumstances.values().sum();
    println!("{total} scram events; wrote {SNAPSHOT}");
    let eras: Vec<i32> = systems.keys().map(|(era, _)| *era).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    println!("by system and five-year era:");
    let mut header = String::new();
    for era in &eras {
        write!(header, "{era:>7}")?;
    }
    println!("  {:<28}{header}", "");
    for system in System::ALL {
        let mut row = String::new();
        for era in &eras {
            write!(row, "{:>7}", systems.get(&(*era, system)).copied().unwrap_or_default())?;
        }
        println!("  {:<28}{row}", system.label());
    }
    println!(
        "by circumstance: {}",
        circumstances
            .iter()
            .map(|(circumstance, count)| format!("{} {count}", circumstance.slug()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("system decided by: {by:?}");

    if episodes {
        print_episodes(&store, &catalog, &set, &triggers)?;
    }
    Ok(())
}

/// Builds the ledger with the episode collector, and prints the episodes against the ledger's
/// `scram` cause.
fn print_episodes(store: &RawStore, catalog: &Catalog, set: &EventSet, triggers: &Triggers) -> anyhow::Result<()> {
    let mut collector = Collector::new(set, &catalog.units, |event| triggers.tags(event));
    let mut scram_cause = (0_u64, 0_u64);
    ledger::build(store, catalog, set, Unknowns::Fail, |days| {
        for day in days.iter().filter(|day| day.cause.as_deref() == Some(SCRAM)) {
            scram_cause.0 += 1;
            scram_cause.1 += day.lost_kwh.unwrap_or_default();
        }
        collector.push(&days);
        Ok(())
    })?;
    let scrams = collector.finish().scrams;
    let with: Vec<_> = scrams.iter().filter_map(|scram| scram.episode.map(|episode| (scram, episode))).collect();
    let mut ends: BTreeMap<String, usize> = BTreeMap::new();
    for (_, episode) in &with {
        *ends.entry(format!("{:?}", episode.end)).or_default() += 1;
    }
    let mut days: Vec<u32> = with.iter().map(|(_, episode)| episode.days).collect();
    days.sort_unstable();
    let gwh = |kwh: u64| kwh / 1_000_000;
    let lost: u64 = with.iter().map(|(_, episode)| episode.lost_kwh).sum();
    println!(
        "{} unit scrams, {} with an episode; ends {ends:?}; median {} days, 90th percentile {}",
        scrams.len(),
        with.len(),
        days.get(days.len() / 2).copied().unwrap_or_default(),
        days.get(days.len() * 9 / 10).copied().unwrap_or_default()
    );
    println!(
        "episodes {} GWh below the pre-scram power; the ledger's `scram` cause {} GWh over {} unit-days",
        gwh(lost),
        gwh(scram_cause.1),
        scram_cause.0
    );
    let returned: u64 = with.iter().filter(|(_, episode)| episode.end == End::Returned).map(|(_, episode)| episode.lost_kwh).sum();
    println!("  of the episodes, {} GWh in those that returned", gwh(returned));
    for system in System::ALL {
        let mine: Vec<_> = with.iter().filter(|(scram, _)| scram.tags.system == system).collect();
        let mut days: Vec<u32> = mine.iter().map(|(_, episode)| episode.days).collect();
        days.sort_unstable();
        println!(
            "  {:<28} {:>4} episodes {:>7} GWh, median {:>3} days",
            system.label(),
            mine.len(),
            gwh(mine.iter().map(|(_, episode)| episode.lost_kwh).sum()),
            days.get(days.len() / 2).copied().unwrap_or_default()
        );
    }
    Ok(())
}
