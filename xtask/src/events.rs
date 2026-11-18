//! `cargo xtask events`: parses every stored Event Notification page and prints what the archive
//! holds, so a template change or a new label shows up before a rebuild meets it (PRD-0003 T-001),
//! how well the update markers parse (T-002), and who the events are about, with every facility the
//! catalogs don't know (T-004).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, ensure};
use chrono::Datelike as _;

use downpower_ingest::events::{Category, Event, Parser, Site, UpdateKind};
use downpower_ingest::facilities::{Facilities, Identity};
use downpower_ingest::ledger::Unknowns;
use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::units::Units;

/// Failures printed in full; the rest are counted.
const SHOWN: usize = 20;

pub fn run(data_dir: &Path) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let parser = Parser::new()?;

    let mut seen = HashSet::new();
    let mut pages = 0_usize;
    let mut empty_by_year: BTreeMap<i32, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    // Every parsed copy of each event, with the date of the page it came from.
    let mut copies: HashMap<u32, Vec<Event>> = HashMap::new();
    for record in store.records()?.into_iter().filter(|record| record.series == Series::EventNotifications) {
        let Some(hash) = record.sha256 else { continue };
        if !seen.insert(hash.clone()) {
            continue;
        }
        pages += 1;
        match parser.parse(&store.get(&hash)?) {
            Ok(events) if events.is_empty() => *empty_by_year.entry(record.date.year()).or_default() += 1,
            Ok(events) => {
                for event in events {
                    copies.entry(event.number).or_default().push(event);
                }
            }
            Err(error) => failures.push(format!("{}: {error:#}", record.date)),
        }
    }

    let empty: usize = empty_by_year.values().sum();
    println!(
        "{pages} pages: {} with events, {empty} reading \"No events found\", {} failed",
        pages - empty - failures.len(),
        failures.len()
    );
    println!("empty pages by year: {empty_by_year:?}");
    let blocks: usize = copies.values().map(Vec::len).sum();
    let repeated = copies.values().filter(|copies| copies.len() > 1).count();
    let differing = copies.values().filter(|copies| copies.windows(2).any(|pair| pair[0] != pair[1])).count();
    println!(
        "{blocks} event blocks, {} distinct events; {repeated} on more than one page, {differing} of them differing",
        copies.len()
    );

    // The copy fetched last stands for each event here, close to the event set's rule (T-003).
    let events: Vec<&Event> = copies.values().filter_map(|copies| copies.last()).collect();
    // Events, and events with no 10 CFR section, by category.
    let mut categories: BTreeMap<Category, (usize, usize)> = BTreeMap::new();
    for event in &events {
        let (all, unsectioned) = categories.entry(event.category).or_default();
        *all += 1;
        *unsectioned += usize::from(event.sections.is_empty());
    }
    println!("by category (events, with no 10 CFR section):");
    for (category, (all, unsectioned)) in &categories {
        println!("  {:<46} {all:>6} {unsectioned:>6}", category.label());
    }
    let count = |test: fn(&Event) -> bool| events.iter().filter(|event| test(event)).count();
    println!(
        "retracted {}, no title {}, no 10 CFR section {}, no emergency class {}, blank event time zone {}",
        count(|event| event.retracted),
        count(|event| event.title.is_none()),
        count(|event| event.sections.is_empty()),
        count(|event| event.class.is_none()),
        count(|event| event.occurred.zone.is_none()),
    );

    let mut untitled: Vec<u32> = events.iter().filter(|event| event.title.is_none()).map(|event| event.number).collect();
    untitled.sort_unstable();
    println!("no title, the first {SHOWN}: {:?}", &untitled[..untitled.len().min(SHOWN)]);

    if let (Some(first), Some(last)) = (copies.keys().min(), copies.keys().max()) {
        let span = last - first + 1;
        println!("event numbers {first} to {last}: {span} numbers, {} unused", span as usize - copies.len());
    }

    timeline(&events);
    let unknown = identity(&events)?;

    for failure in failures.iter().take(SHOWN) {
        println!("FAILED {failure}");
    }
    if !failures.is_empty() {
        bail!("{} of {pages} event pages failed to parse", failures.len());
    }
    if !unknown.is_empty() {
        bail!("{} facilities the catalogs don't know; add them to catalog/units.csv or catalog/facilities.csv", unknown.len());
    }
    Ok(())
}

/// Who the events are about: identified events by kind, the licensee groups, and every facility the
/// catalogs don't know, which it returns.
fn identity(events: &[&Event]) -> anyhow::Result<BTreeMap<String, u64>> {
    let facilities = Facilities::catalog(&Units::catalog()?)?;
    let set: BTreeMap<u32, Event> = events.iter().map(|event| (event.number, (*event).clone())).collect();
    let mut unknown = BTreeMap::new();
    let identities = facilities.identify_all(&set, Unknowns::Quarantine, &mut unknown)?;

    let (mut units, mut sites, mut facility, mut licensee) = (0_usize, 0_usize, 0_usize, 0_usize);
    // Each licensee group's names as its events spell them, ignoring case.
    let mut groups: HashMap<(&str, &str), HashSet<String>> = HashMap::new();
    for (number, identity) in &identities {
        match identity {
            Identity::Reactor { dockets } if dockets.is_empty() => sites += 1,
            Identity::Reactor { .. } => units += 1,
            Identity::Facility { .. } => facility += 1,
            Identity::Licensee { state, license, .. } => {
                licensee += 1;
                let names = groups.entry((state, license)).or_default();
                if let Site::Licensee(site) = &set[number].site {
                    names.extend(site.name.as_deref().map(str::to_uppercase));
                }
            }
        }
    }
    let quarantined = usize::try_from(unknown.values().sum::<u64>())?;
    println!(
        "identified {} of {} events: {units} reactor events by unit, {sites} by site alone, {facility} at a catalogued facility, {licensee} by licensee; {} stay text",
        identities.len(),
        set.len(),
        set.len() - identities.len() - quarantined
    );
    println!(
        "{} licensee groups by (state, license number), {} of them under more than one name",
        groups.len(),
        groups.values().filter(|names| names.len() > 1).count()
    );
    let mut by_uses: Vec<(&String, &u64)> = unknown.iter().collect();
    by_uses.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (name, uses) in by_uses.iter().take(SHOWN) {
        println!("UNKNOWN {name}: {uses} events");
    }
    Ok(unknown)
}

/// The update markers: kinds, how often each field parses, and the banner against the timeline.
fn timeline(events: &[&Event]) {
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let (mut markers, mut dated, mut timed, mut named, mut outside) = (0_usize, 0_usize, 0_usize, 0_usize, 0_usize);
    let mut undated = Vec::new();
    for event in events {
        for update in &event.updates {
            *kinds.entry(format!("{:?}", update.kind)).or_default() += 1;
            if update.kind == UpdateKind::Note {
                continue;
            }
            markers += 1;
            dated += usize::from(update.date.is_some());
            timed += usize::from(update.time.is_some());
            named += usize::from(update.from.is_some() && update.to.is_some());
            if let Some(date) = update.date {
                // Allow a day either side for time zones and late typing.
                outside += usize::from(date < event.notified.date.pred_opt().unwrap_or(date) || date > event.updated.and_then(|updated| updated.succ_opt()).unwrap_or(date));
            } else if undated.len() < SHOWN {
                undated.push(update.marker.clone());
            }
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "shares for a human to read")]
    let share = |count: usize| 100.0 * count as f64 / markers.max(1) as f64;
    println!("markers by kind: {kinds:?}");
    println!(
        "{markers} update and retraction markers: date {:.1}%, time {:.1}%, from and to {:.1}%; {outside} dated outside notification to last update",
        share(dated),
        share(timed),
        share(named)
    );
    let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
    for event in events.iter().filter(|event| !event.updates.is_empty()) {
        *sizes.entry(event.updates.len().min(3)).or_default() += 1;
    }
    println!(
        "events by update count (3 means 3 or more): {sizes:?}; revision notes {}",
        events.iter().filter(|event| event.revised.is_some()).count()
    );
    let retraction = |event: &Event| event.updates.iter().any(|update| update.kind == UpdateKind::Retraction);
    let mut agreement: BTreeMap<(bool, bool), usize> = BTreeMap::new();
    for event in events {
        *agreement.entry((event.retracted, retraction(event))).or_default() += 1;
    }
    println!("(banner, a retraction marker) => events: {agreement:?}");
    println!("markers with no date, the first {SHOWN}:");
    for marker in &undated {
        println!("  {marker}");
    }
}
