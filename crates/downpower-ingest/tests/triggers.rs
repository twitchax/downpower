//! Scram triggers (PRD-0007 uat-001 to uat-003): the committed rules scored against the sample
//! labeled by hand before any rule existed, phrase rules on fixture text, and the catalog's checks.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use pretty_assertions::assert_eq;

use downpower_core::triggers::{Circumstance, System, Tags};
use downpower_ingest::events::{self, Archive, Event, EventSet, Scram};
use downpower_ingest::ledger::{Catalog, Unknowns};
use downpower_ingest::triggers::Triggers;

/// The least share of the sample's labels the rules must agree with, per dimension.
const AGREEMENT: f64 = 0.90;

/// The least share of the validation labels: 50 events drawn after the rules were tuned on the
/// sample, which scored 39 and 43 of 50 the first time (PRD-0007.log.md#t-002).
const VALIDATION: f64 = 0.85;

/// A fixture's events and labels, in label order.
fn labeled(events: &str, labels: &str) -> Vec<(Event, Tags)> {
    let events: BTreeMap<u32, Event> = events.lines().map(|line| serde_json::from_str::<Event>(line).unwrap()).map(|event| (event.number, event)).collect();
    labels
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let number: u32 = fields[0].parse().unwrap();
            let tags = Tags {
                system: System::from_slug(fields[1]).unwrap(),
                circumstance: Circumstance::from_slug(fields[2]).unwrap(),
            };
            (events[&number].clone(), tags)
        })
        .collect()
}

/// How many of `labeled` the committed rules agree with, per dimension, and the misses.
fn agreement(labeled: &[(Event, Tags)]) -> (u32, u32, String) {
    let triggers = Triggers::catalog().unwrap();
    let mut systems = 0_u32;
    let mut circumstances = 0_u32;
    let mut misses = String::new();
    for (event, label) in labeled {
        let tagged = triggers.tags(event);
        systems += u32::from(tagged.system == label.system);
        circumstances += u32::from(tagged.circumstance == label.circumstance);
        if tagged != *label {
            let _ = writeln!(
                misses,
                "EN {}: labeled {}/{}, tagged {}/{}",
                event.number,
                label.system.slug(),
                label.circumstance.slug(),
                tagged.system.slug(),
                tagged.circumstance.slug()
            );
        }
    }
    (systems, circumstances, misses)
}

#[test]
fn uat_scram_triggers_sample_agrees_with_the_hand_labels() {
    for (name, events, labels, count, least) in [
        ("sample", include_str!("fixtures/scrams/sample.jsonl"), include_str!("fixtures/scrams/labeled.tsv"), 150_u32, AGREEMENT),
        (
            "validation",
            include_str!("fixtures/scrams/validation.jsonl"),
            include_str!("fixtures/scrams/validation.tsv"),
            50,
            VALIDATION,
        ),
    ] {
        let labeled = labeled(events, labels);
        assert_eq!(labeled.len(), count as usize, "{name}");
        let (systems, circumstances, misses) = agreement(&labeled);
        let share = |agreed: u32| f64::from(agreed) / f64::from(count);
        println!("{name}: system {systems}/{count}, circumstance {circumstances}/{count}\n{misses}");
        assert!(share(systems) >= least, "{name} system agreement {systems}/{count}\n{misses}");
        assert!(share(circumstances) >= least, "{name} circumstance agreement {circumstances}/{count}\n{misses}");
    }
}

/// The fixture page's first event rewritten to carry `title` and `text` alone, scrammed.
fn scram_with(title: &str, text: &str) -> Event {
    let mut event = events::parse(include_bytes!("fixtures/events/2009-08-24.html")).unwrap().remove(0);
    event.title = Some(title.to_owned());
    event.text = format!("{title}\n{text}");
    event.updates.clear();
    event
}

fn tags_of(title: &str, text: &str) -> Tags {
    Triggers::catalog().unwrap().tags(&scram_with(title, text))
}

#[test]
fn uat_scram_triggers_rules_take_the_root_over_the_signal_it_caused() {
    let cases = [
        (
            "AUTOMATIC REACTOR TRIP",
            "The reactor tripped on a turbine trip caused by a loss of offsite power when the 345 kV line faulted.",
            System::OffsitePower,
        ),
        (
            "MANUAL REACTOR TRIP",
            "Operators tripped the reactor on lowering steam generator level after the 'A' main feed pump tripped.",
            System::Feedwater,
        ),
        (
            "AUTOMATIC REACTOR SCRAM",
            "A generator lockout from a fault on the main transformer caused a turbine trip and reactor scram.",
            System::Electrical,
        ),
        (
            "AUTOMATIC REACTOR TRIP",
            "The reactor tripped on low-low steam generator level following a loss of condenser vacuum.",
            System::CoolingWater,
        ),
        (
            "AUTOMATIC REACTOR TRIP ON TURBINE TRIP",
            "The cause of the turbine trip is under investigation.",
            System::TurbineGenerator,
        ),
        (
            "REACTOR TRIP DUE TO LOSS OF REACTOR COOLANT PUMP",
            "The 1B reactor coolant pump tripped, causing a low flow trip.",
            System::ReactorCoolant,
        ),
        ("AUTOMATIC REACTOR TRIP", "The cause of the trip is under investigation.", System::NotNamed),
    ];
    for (title, text, system) in cases {
        assert_eq!(tags_of(title, text).system, system, "{text}");
    }
}

#[test]
fn uat_scram_triggers_rules_tag_the_circumstance() {
    assert_eq!(
        tags_of(
            "AUTOMATIC REACTOR TRIP DURING TESTING",
            "During surveillance testing of the turbine, the turbine tripped unexpectedly causing a reactor trip."
        )
        .circumstance,
        Circumstance::Testing
    );
    assert_eq!(
        tags_of("AUTOMATIC REACTOR SCRAM", "A lightning strike in the switchyard caused a loss of offsite power and a reactor scram.").circumstance,
        Circumstance::Weather
    );
    assert_eq!(
        tags_of("MANUAL REACTOR TRIP", "Operators tripped the reactor after a feed pump trip.").circumstance,
        Circumstance::Unnamed
    );
}

#[test]
fn uat_scram_triggers_rules_skip_the_known_misfires() {
    // A PWR's coolant loop isn't a loss of offsite power.
    assert_ne!(
        tags_of("MANUAL REACTOR TRIP", "Operators tripped the reactor when the loop 1 main steam isolation valve failed shut.").system,
        System::OffsitePower
    );
    // The transformer the buses transferred to after the trip didn't cause it.
    assert_eq!(
        tags_of(
            "AUTOMATIC REACTOR TRIP",
            "The reactor tripped on low-low steam generator level after a feed pump trip. All buses transferred to the startup transformer as designed."
        )
        .system,
        System::Feedwater
    );
    // Auxiliary feedwater starting is the response, not the trigger.
    assert_eq!(
        tags_of(
            "AUTOMATIC REACTOR TRIP",
            "The reactor tripped on a turbine trip. Auxiliary feedwater started as expected and steam generator levels are maintained."
        )
        .system,
        System::TurbineGenerator
    );
    // Seismic monitoring isn't weather.
    assert_eq!(
        tags_of("AUTOMATIC REACTOR TRIP", "The reactor tripped on a turbine trip while seismic monitoring was out of service.").circumstance,
        Circumstance::Unnamed
    );
}

/// The fixture page's events, identified, with EN 45290 given a scram.
fn fixture_events() -> EventSet {
    let catalog = Catalog::embedded().unwrap();
    let mut events: BTreeMap<u32, Event> = events::parse(include_bytes!("fixtures/events/2009-08-24.html"))
        .unwrap()
        .into_iter()
        .map(|event| (event.number, event))
        .collect();
    events.get_mut(&45290).unwrap().units[0].scram = Some(Scram::AutoReactor);
    events.get_mut(&45289).unwrap().units.iter_mut().for_each(|unit| unit.scram = Some(Scram::No));
    let identities = catalog.facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap();
    EventSet {
        events,
        identities,
        pages: BTreeMap::new(),
        archive: Archive::default(),
    }
}

const RULES: &str = "near = 0\ncause = 'caus'\ncue = '.'\nstrip = []\nsignal = []\n[[system]]\nvalue = \"feedwater\"\npattern = 'feed'\n[[circumstance]]\nvalue = \"testing\"\npattern = 'test'\n";

#[test]
fn uat_scram_triggers_catalog_rejects_what_it_should() {
    Triggers::catalog().unwrap().check(&fixture_events()).unwrap();
    let assign = |entry: &str| format!("{RULES}[[assign]]\n{entry}\n");
    Triggers::parse(&assign("number = 45290\nsystem = \"feedwater\"\nwhy = \"the feed pump\"")).unwrap();
    assert!(Triggers::parse(&RULES.replace("'feed'", "'('")).is_err(), "a bad pattern");
    assert!(Triggers::parse(&RULES.replace("\"feedwater\"", "\"fire\"")).is_err(), "an unknown value");
    assert!(Triggers::parse(&assign("number = 45290\nsystem = \"feedwater\"\nwhy = \" \"")).is_err(), "no reason");
    assert!(Triggers::parse(&assign("number = 45290\nwhy = \"nothing assigned\"")).is_err(), "nothing assigned");
    assert!(
        Triggers::parse(&format!(
            "{}[[assign]]\nnumber = 45290\nsystem = \"electrical\"\nwhy = \"b\"\n",
            assign("number = 45290\nsystem = \"feedwater\"\nwhy = \"a\"")
        ))
        .is_err(),
        "assigned twice"
    );
    // EN 45289 scrammed nothing, so an entry for it is a mistake.
    let triggers = Triggers::parse(&assign("number = 45289\nsystem = \"feedwater\"\nwhy = \"not a scram\"")).unwrap();
    assert!(triggers.check(&fixture_events()).is_err(), "a listed event that isn't a scram");
    let triggers = Triggers::parse(&assign("number = 1\nsystem = \"feedwater\"\nwhy = \"no such event\"")).unwrap();
    assert!(triggers.check(&fixture_events()).is_err(), "a listed event the archive lacks");
}

#[test]
fn uat_scram_triggers_catalog_assignments_override_the_rules() {
    let triggers = Triggers::parse(&format!("{RULES}[[assign]]\nnumber = 45290\ncircumstance = \"weather\"\nwhy = \"the update names the storm\"\n")).unwrap();
    let events = fixture_events();
    let mut event = events.events[&45290].clone();
    event.text = "The reactor tripped after a feed pump trip.".to_owned();
    assert_eq!(
        triggers.tags(&event),
        Tags {
            system: System::Feedwater,
            circumstance: Circumstance::Weather
        }
    );
}
