//! `cargo xtask scrams`: both scram checks over the stored archive (PRD-0003 T-007 and T-008). The
//! internal one gives both directions' agreement by year and the cases where the two NRC sources
//! disagree; the dataset's gives its agreement by year and the scrams one side lacks.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context as _, ensure};
use chrono::Datelike as _;

use downpower_ingest::events::EventSet;
use downpower_ingest::ledger::{self, Catalog, Unknowns};
use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::scram_dataset::{self, Agreement};
use downpower_ingest::scrams::{Check, Found, Shown};

pub fn run(data_dir: &Path, examples: usize) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let catalog = Catalog::embedded()?;
    let set = EventSet::build(&store, &catalog.facilities, Unknowns::Fail)?;
    let mut check = Check::new(&set);
    ledger::build(&store, &catalog, &set, Unknowns::Fail, |days| {
        check.observe(&days);
        Ok(())
    })?;
    let cases = check.finish();
    let counts = cases.counts();
    let names: HashMap<&str, &str> = catalog.units.all().iter().map(|unit| (unit.docket.as_str(), unit.name.as_str())).collect();
    let name = |docket: &str| names.get(docket).copied().unwrap_or(docket).to_owned();

    let (events, reports) = (counts.events, counts.reports);
    println!("EN scrams the status reports show: {}", agreement(events.agreed(), events.checked()));
    println!("  {events:?}");
    println!("status-report scrams an EN reports: {}", agreement(reports.agreed(), reports.checked()));
    println!("  {reports:?}");

    let mut years: BTreeMap<i32, [(u64, u64); 2]> = BTreeMap::new();
    for case in &cases.events {
        let year = &mut years.entry(case.date.year()).or_default()[0];
        year.0 += u64::from(matches!(case.outcome, Shown::Dropped | Shown::Named));
        year.1 += u64::from(matches!(case.outcome, Shown::Dropped | Shown::Named | Shown::Missed));
    }
    for case in &cases.reports {
        let year = &mut years.entry(case.date.year()).or_default()[1];
        year.0 += u64::from(case.outcome == Found::Event);
        year.1 += u64::from(case.outcome != Found::Retracted);
    }
    println!("by year (EN scrams the reports show; report scrams an EN reports):");
    for (year, [shown, found]) in &years {
        println!("  {year}  {:<22} {}", agreement(shown.0, shown.1), agreement(found.0, found.1));
    }

    let missed: Vec<_> = cases.events.iter().filter(|case| case.outcome == Shown::Missed).collect();
    println!("EN scrams the reports miss ({}):", missed.len());
    for case in missed {
        let powers = case.powers.map(|power| power.map_or_else(|| "-".to_owned(), |power| power.to_string()));
        println!(
            "  EN {} {} {:<20} {:?} from {}%, reported {} then {}",
            case.number,
            case.date,
            name(&case.docket),
            case.scram,
            case.initial_power,
            powers[0],
            powers[1]
        );
    }
    for (outcome, label) in [(Found::Missed, "no event on the unit"), (Found::NoScramCode, "events on the unit, none with a scram")] {
        let listed: Vec<_> = cases.reports.iter().filter(|case| case.outcome == outcome).collect();
        println!("status-report scrams with {label}, the first {examples} of {}:", listed.len());
        for case in listed.iter().take(examples) {
            println!(
                "  {} {:<20} reported {} {:?} {:?}",
                case.date,
                name(&case.docket),
                case.reported,
                case.events,
                case.reason.as_deref().unwrap_or_default()
            );
        }
    }
    dataset(&store, &set, &name, examples)
}

/// The newest stored scram dataset against our EN scrams.
fn dataset(store: &RawStore, events: &EventSet, name: &dyn Fn(&str) -> String, examples: usize) -> anyhow::Result<()> {
    let Some(record) = store.latest(Series::ScramDataset)?.into_values().rev().find(|record| record.sha256.is_some()) else {
        println!("no scram dataset stored; run `downpower fetch --series scram-dataset`");
        return Ok(());
    };
    let hash = record.sha256.context("the record was chosen for its hash")?;
    let rows = scram_dataset::parse(&store.get(&hash)?)?;
    let reconciled = scram_dataset::reconcile(&rows, events).context("the scram dataset has no rows")?;
    let counts = reconciled.counts();
    println!(
        "NRC scram dataset (fetched {}, {} rows, {} to {}), scrams both list: {}",
        record.date,
        rows.len(),
        counts.first,
        counts.last,
        agreement(counts.agreed(), counts.checked())
    );
    println!("  {counts:?}");
    let mut years: BTreeMap<i32, (u64, u64)> = BTreeMap::new();
    for case in &reconciled.cases {
        let year = years.entry(case.date.year()).or_default();
        year.0 += u64::from(case.outcome == Agreement::Agree);
        year.1 += u64::from(matches!(case.outcome, Agreement::Agree | Agreement::DatasetOnly | Agreement::OursOnly));
    }
    println!("by year (scrams both list):");
    for (year, (agreed, checked)) in &years {
        println!("  {year}  {}", agreement(*agreed, *checked));
    }
    for (outcomes, label) in [
        (&[Agreement::DatasetOnly][..], "only the dataset lists"),
        (&[Agreement::OursOnly][..], "only our Event Notifications hold"),
        (&[Agreement::NrcRetracted, Agreement::WeRetracted, Agreement::NotCritical][..], "known differences"),
    ] {
        let listed: Vec<_> = reconciled.cases.iter().filter(|case| outcomes.contains(&case.outcome)).collect();
        println!("scrams {label}, the first {examples} of {}:", listed.len());
        for case in listed.iter().take(examples) {
            let codes = events.events.get(&case.event).map(|event| {
                let units = event
                    .units
                    .iter()
                    .map(|unit| format!("{:?} {}from {}%", unit.scram, if unit.critical { "" } else { "not critical " }, unit.initial_power));
                units.collect::<Vec<_>>().join(", ")
            });
            println!(
                "  EN {} {} {:<20} {:?} ours: {}",
                case.event,
                case.date,
                name(&case.docket),
                case.outcome,
                codes.as_deref().unwrap_or("no such event")
            );
        }
    }
    Ok(())
}

/// `part` of `whole` in words: `1,208 of 1,787 (67.6%)`.
fn agreement(part: u64, whole: u64) -> String {
    #[allow(clippy::cast_precision_loss, reason = "a share for a human to read")]
    let percent = if whole == 0 { 0.0 } else { part as f64 / whole as f64 * 100.0 };
    format!("{part} of {whole} ({percent:.1}%)")
}
