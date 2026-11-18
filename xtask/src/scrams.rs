//! `cargo xtask scrams`: the internal scram check over the stored archive (PRD-0003 T-007), with
//! both directions' agreement by year and the cases where the two NRC sources disagree.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::ensure;
use chrono::Datelike as _;

use downpower_ingest::events::EventSet;
use downpower_ingest::ledger::{self, Catalog, Unknowns};
use downpower_ingest::raw::RawStore;
use downpower_ingest::scrams::{Check, Found, Shown};

pub fn run(data_dir: &Path, examples: usize) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let catalog = Catalog::embedded()?;
    let events = EventSet::build(&store, &catalog.facilities, Unknowns::Fail)?;
    let mut check = Check::new(&events);
    ledger::build(&store, &catalog, &events, Unknowns::Fail, |days| {
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
    Ok(())
}

/// `part` of `whole` in words: `1,208 of 1,787 (67.6%)`.
fn agreement(part: u64, whole: u64) -> String {
    #[allow(clippy::cast_precision_loss, reason = "a share for a human to read")]
    let percent = if whole == 0 { 0.0 } else { part as f64 / whole as f64 * 100.0 };
    format!("{part} of {whole} ({percent:.1}%)")
}
