//! The Event Notification parser against live NRC pages from every era (PRD-0003 uat-001). The
//! fixtures are public-domain pages copied byte for byte out of the raw store, chosen so that
//! together they hold every category, both header templates, retractions, revision imports, a
//! fuel cycle facility's comments, NUL placeholders and a page with no events.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use pretty_assertions::assert_eq;

use downpower_ingest::events::{self, Archive, Category, EmergencyClass, Event, EventSet, Facility, Licensee, Scram, Section, Site, Stamp, UnitInfo, Update};
use downpower_ingest::ledger::Unknowns;
use downpower_ingest::raw::{RawStore, Record, Series, Source};

macro_rules! fixture {
    ($date:literal) => {
        include_bytes!(concat!("fixtures/events/", $date, ".html")).as_slice()
    };
}

/// Every fixture and its event numbers, in page order.
const PAGES: [(&str, &[u8], &[u32]); 8] = [
    ("1999-01-15", fixture!("1999-01-15"), &[35275, 35267, 35268, 35269, 35270, 35271, 35272, 35273, 35278, 35599]),
    ("1999-02-02", fixture!("1999-02-02"), &[]),
    ("2002-10-14", fixture!("2002-10-14"), &[39290, 39282, 39283, 39314]),
    ("2009-08-24", fixture!("2009-08-24"), &[45459, 45285, 45286, 45287, 45289, 45290, 45293, 45294, 45295]),
    ("2010-05-26", fixture!("2010-05-26"), &[45957, 46009, 46890]),
    ("2012-01-09", fixture!("2012-01-09"), &[47580, 47581, 47578]),
    ("2020-10-20", fixture!("2020-10-20"), &[54959, 54960, 54962, 54964, 54965, 55027]),
    ("2026-02-11", fixture!("2026-02-11"), &[58149, 58151, 58160]),
];

fn page(date: &str) -> Vec<Event> {
    let (_, bytes, _) = PAGES.iter().find(|(day, ..)| *day == date).unwrap();
    events::parse(bytes).unwrap()
}

fn event(date: &str, number: u32) -> Event {
    page(date).into_iter().find(|event| event.number == number).unwrap_or_else(|| panic!("no event {number} on {date}"))
}

fn stamp(date: &str, time: &str, zone: &str) -> Stamp {
    Stamp {
        date: date.parse().unwrap(),
        time: NaiveTime::parse_from_str(time, "%H:%M").unwrap(),
        zone: Some(zone.to_owned()),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn uat_events_parse_every_page_in_page_order() {
    for (date, bytes, numbers) in PAGES {
        let events = events::parse(bytes).unwrap();

        assert_eq!(events.iter().map(|event| event.number).collect::<Vec<_>>(), numbers, "{date}");
        assert!(events.iter().all(|event| !event.text.is_empty() && event.notified.date <= event.updated), "{date}");
    }
}

#[test]
fn uat_events_parse_every_category_under_its_template() {
    let events: Vec<Event> = PAGES.iter().flat_map(|(_, bytes, _)| events::parse(bytes).unwrap()).collect();

    let categories: BTreeSet<Category> = events.iter().map(|event| event.category).collect();
    assert_eq!(categories, Category::ALL.into_iter().collect());
    for event in &events {
        let reactor = event.category == Category::PowerReactor;
        let facility = matches!(event.category, Category::PowerReactor | Category::FuelCycleFacility);
        assert_eq!(matches!(event.site, Site::Facility(_)), facility, "{}", event.number);
        assert_eq!(!event.units.is_empty(), reactor, "{}", event.number);
    }
}

#[test]
fn uat_events_parse_an_empty_page_as_no_events() {
    assert_eq!(page("1999-02-02"), []);
}

#[test]
fn uat_events_parse_a_1999_fuel_cycle_facility_with_comments() {
    let event = event("1999-01-15", 35275);

    assert_eq!(
        Event {
            text: String::new(),
            updates: vec![],
            ..event.clone()
        },
        Event {
            number: 35275,
            category: Category::FuelCycleFacility,
            retracted: false,
            site: Site::Facility(Facility {
                name: "PORTSMOUTH GASEOUS DIFFUSION PLANT".to_owned(),
                units: vec![],
                rx_type: Some("URANIUM ENRICHMENT FACILITY".to_owned()),
                comments: Some("2 DEMOCRACY CENTER\n6903 ROCKLEDGE DRIVE\nBETHESDA, MD 20817    (301)564-3200".to_owned()),
            }),
            region: 3,
            state: Some("OH".to_owned()),
            notified_by: Some("RICHARD LARSON".to_owned()),
            ops_officer: Some("STEVE SANDIN".to_owned()),
            notified: stamp("1999-01-15", "18:08", "ET"),
            occurred: stamp("1999-01-15", "14:10", "EST"),
            updated: NaiveDate::from_ymd_opt(1999, 1, 25).unwrap(),
            class: Some(EmergencyClass::NonEmergency),
            sections: vec![],
            persons: strings(&["JIM GAVULA (R3)", "JOHN SURMEIER (NMSS)", "FRANK CONGEL (IRO)"]),
            units: vec![],
            title: Some("4-HOUR NRC 91-01 BULLETIN NOTIFICATION INVOLVING ABANDONED EQUIPMENT IN THE X-326 PROCESS BUILDING".to_owned()),
            text: String::new(),
            updates: vec![],
            revised: None,
        }
    );
    assert!(event.text.starts_with(
        "4-HOUR NRC 91-01 BULLETIN NOTIFICATION INVOLVING ABANDONED EQUIPMENT IN THE X-326 PROCESS BUILDING\n\n\
         THE FOLLOWING TEXT IS A PORTION OF A FACSIMILE RECEIVED FROM PORTSMOUTH:\n\n\"ON JANUARY 15, 1999,"
    ));
}

#[test]
fn uat_events_parse_a_1999_research_reactor_under_the_licensee_template() {
    let event = event("1999-01-15", 35267);

    assert_eq!(
        event.site,
        Site::Licensee(Licensee {
            rep_org: Some("NATIONAL INST OF STANDARDS & TECH".to_owned()),
            name: Some("U. S. DEPT. OF COMMERCE".to_owned()),
            city: Some("GAITHERSBURG".to_owned()),
            county: Some("MONTGOMERY".to_owned()),
            license: Some("TR-5".to_owned()),
            agreement: false,
            docket: Some("05000184".to_owned()),
        })
    );
    assert_eq!((event.category, event.region, event.state.as_deref()), (Category::ResearchReactor, 1, Some("MD")));
    assert_eq!(event.persons, strings(&["GENE KELLY (R1)", "DAVID MATTHEWS/NRR (EO)", "FRANK CONGEL (IRO)", "TED MICHAELS/NRR (PM)"]));
    assert_eq!(event.title.as_deref(), Some("LOSS OF COMMERCIAL OFFSITE POWER AND AUTOMATIC REACTOR SHUT DOWN"));
}

#[test]
fn uat_events_parse_a_2010_scram_with_its_unit_table_and_two_sections() {
    let event = event("2010-05-26", 45957);

    assert_eq!(
        event.site,
        Site::Facility(Facility {
            name: "VERMONT YANKEE".to_owned(),
            units: vec![1],
            rx_type: Some("[1] GE-4".to_owned()),
            comments: None,
        })
    );
    assert_eq!((event.notified, event.occurred), (stamp("2010-05-26", "17:25", "ET"), stamp("2010-05-26", "15:26", "EDT")));
    assert_eq!(
        event.sections,
        [
            Section {
                code: Some("50.72(b)(2)(iv)(B)".to_owned()),
                description: "RPS ACTUATION - CRITICAL".to_owned(),
            },
            Section {
                code: Some("50.72(b)(3)(iv)(A)".to_owned()),
                description: "VALID SPECIF SYS ACTUATION".to_owned(),
            },
        ]
    );
    assert_eq!(
        event.units,
        [UnitInfo {
            unit: 1,
            scram: Some(Scram::Auto),
            critical: true,
            initial_power: 72,
            initial_mode: Some("Power Operation".to_owned()),
            current_power: 0,
            current_mode: Some("Hot Shutdown".to_owned()),
        }]
    );
    assert_eq!(event.title.as_deref(), Some("AUTOMATIC REACTOR SCRAM WHILE INCREASING POWER"));
}

#[test]
fn uat_events_parse_a_2020_retraction_and_its_revision_import() {
    let event = event("2020-10-20", 54960);

    assert!(event.retracted);
    assert_eq!(
        event.sections,
        [Section {
            code: None,
            description: "Agreement State".to_owned()
        }]
    );
    assert_eq!(event.title.as_deref(), Some("AGREEMENT STATE REPORT - DAMAGED GAUGE"));
    assert!(
        event
            .text
            .starts_with("EN Revision Imported Date: 12/8/2020\n\nEN Revision Text: AGREEMENT STATE REPORT - DAMAGED GAUGE\n\n")
    );
    let retracted: Vec<u32> = page("2020-10-20").iter().filter(|event| event.retracted).map(|event| event.number).collect();
    assert_eq!(retracted, [54960]);
}

#[test]
fn uat_events_parse_2026_nul_placeholders_as_blank() {
    for number in [58149, 58151] {
        let Site::Licensee(licensee) = event("2026-02-11", number).site else {
            panic!("{number} is not a licensee")
        };

        assert_eq!(licensee.county, None, "{number}");
    }
}

#[test]
fn uat_events_parse_a_2026_non_power_reactor_in_region_0() {
    let event = event("2026-02-11", 58160);

    assert_eq!((event.category, event.region), (Category::NonPowerReactor, 0));
    assert_eq!((event.notified, event.occurred), (stamp("2026-02-10", "16:48", "ET"), stamp("2026-01-21", "14:00", "MST")));
    let Site::Licensee(licensee) = event.site else { panic!("not a licensee") };
    assert_eq!((licensee.license.as_deref(), licensee.docket.as_deref(), licensee.agreement), (Some("R-102"), Some("05000252"), true));
    assert_eq!(event.title.as_deref(), Some("TECHNICAL SPECIFICATION VIOLATION"));
}

/// An update's fields on one line: kind, date, time and zone, then from and to.
fn summary(update: &Update) -> String {
    let or_dash = |value: Option<String>| value.unwrap_or_else(|| "-".to_owned());
    format!(
        "{:?} {} {} {} | {} | {}",
        update.kind,
        or_dash(update.date.map(|date| date.to_string())),
        or_dash(update.time.map(|time| time.format("%H:%M").to_string())),
        or_dash(update.zone.clone()),
        or_dash(update.from.clone()),
        or_dash(update.to.clone()),
    )
}

fn summaries(event: &Event) -> Vec<String> {
    event.updates.iter().map(summary).collect()
}

#[test]
fn uat_event_updates_parse_four_1999_updates_in_three_formats() {
    let event = event("1999-01-15", 35275);

    assert_eq!(
        summaries(&event),
        [
            "Update 1999-01-18 19:55 - | LARSON | WEAVER",
            "Update 1999-01-20 16:43 - | HALCOMB | WEAVER",
            "Update 1999-01-21 18:47 EST | SISSLER | MacKINNON",
            "Update 1999-01-25 12:55 EST | RICHARD LARSON | FANGIE JONES",
        ]
    );
    assert!(
        event.updates[0]
            .text
            .starts_with("THE FOLLOWING TEXT IS A PORTION OF A FACSIMILE RECEIVED FROM PORTSMOUTH:\n\n\"THIS IS REPORTABLE")
    );
    assert!(!event.updates[0].text.contains("* * *"));
    assert!(event.updates[3].text.ends_with("IRO (Frank Congel)."));
}

#[test]
fn uat_event_updates_parse_long_star_runs_email_updates_and_at_signs() {
    assert_eq!(summaries(&event("1999-01-15", 35599)), ["Update 1999-04-27 16:53 - | JEFF HAVENER | TROCINE"]);
    assert_eq!(
        summaries(&event("2009-08-24", 45459)),
        [
            "Update 2009-12-01 16:42 EST | PAUL STEPANTSCHENKO | DONG PARK",
            "Update 2011-04-01 16:13 EST | TOM HORNER | DONG PARK",
            "Update 2011-12-01 16:11 EST | TOM HORNER | VINCE KLCO",
        ]
    );
    assert_eq!(summaries(&event("2009-08-24", 45287)), ["Update 2009-08-25 15:34 - | GREG MILLER | HOWIE CROUCH"]);
    assert_eq!(summaries(&event("1999-01-15", 35267)), Vec::<String>::new());
}

#[test]
fn uat_event_updates_mark_retractions_by_the_banner() {
    let retraction = event("2020-10-20", 54960);
    let banner_only = event("1999-01-15", 35269);

    assert!(retraction.retracted && banner_only.retracted);
    assert_eq!(summaries(&retraction), ["Retraction 2020-12-08 09:22 EST | ASFAW FENTA | SOLOMON SAHLE"]);
    assert_eq!(summaries(&banner_only), ["Update 1999-01-20 17:01 - | SWAFFORD | WEAVER"]);
}

#[test]
fn uat_event_updates_keep_a_revision_import_as_a_note() {
    let event = event("2020-10-20", 54960);

    assert_eq!(event.revised, NaiveDate::from_ymd_opt(2020, 12, 8));
    assert!(event.updates.iter().all(|update| !update.marker.contains("EN Revision")));
    let revised: Vec<u32> = page("2020-10-20").iter().filter(|event| event.revised.is_some()).map(|event| event.number).collect();
    assert!(revised.contains(&54960));
}

/// Records `bytes` (or a 404 for `None`) as the event page for `date`, fetched at `fetched`.
fn put(store: &RawStore, date: &str, fetched: &str, bytes: Option<&[u8]>) {
    let at: DateTime<Utc> = fetched.parse().unwrap();
    let record = Record {
        source: Source::Nrc,
        series: Series::EventNotifications,
        date: date.parse().unwrap(),
        url: String::new(),
        status: if bytes.is_some() { 200 } else { 404 },
        captured_at: at,
        fetched_at: at,
        sha256: bytes.map(|bytes| store.put(bytes).unwrap()),
        len: bytes.map(|bytes| bytes.len() as u64),
    };
    store.append(&record).unwrap();
}

const EARLIER: &str = "2026-11-13T00:00:00Z";
const LATER: &str = "2026-11-14T00:00:00Z";

/// The 2020-10-20 page with EN 54960's licensee renamed, as a re-rendered page after an NRC edit.
fn edited() -> Vec<u8> {
    let page = String::from_utf8(fixture!("2020-10-20").to_vec()).unwrap();
    page.replacen("Hillis-Carnes Engineering Associates, Inc.", "Hillis-Carnes Engineering", 1).into_bytes()
}

fn licensee_of(set: &EventSet, number: u32) -> Option<String> {
    match &set.events[&number].site {
        Site::Licensee(licensee) => licensee.name.clone(),
        Site::Facility(_) => None,
    }
}

#[test]
fn event_set_holds_every_event_once_with_the_archive_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();
    put(&store, "2020-10-20", EARLIER, Some(fixture!("2020-10-20")));
    put(&store, "2026-02-11", EARLIER, Some(fixture!("2026-02-11")));
    put(&store, "1999-02-02", EARLIER, Some(fixture!("1999-02-02")));
    put(&store, "1999-02-03", EARLIER, None);

    let set = EventSet::build(&store, Unknowns::Fail).unwrap();

    assert_eq!(set.events.keys().copied().collect::<Vec<_>>(), [54959, 54960, 54962, 54964, 54965, 55027, 58149, 58151, 58160]);
    assert!(set.events.iter().all(|(number, event)| *number == event.number));
    assert_eq!(
        set.archive,
        Archive {
            pages: 3,
            empty: 1,
            events: 9,
            failed: vec![],
        }
    );
}

#[test]
fn event_set_takes_each_event_from_the_page_fetched_last() {
    for (original, edit, expected) in [(EARLIER, LATER, "Hillis-Carnes Engineering"), (LATER, EARLIER, "Hillis-Carnes Engineering Associates, Inc.")] {
        let tmp = tempfile::tempdir().unwrap();
        let store = RawStore::open(tmp.path()).unwrap();
        put(&store, "2020-10-20", original, Some(fixture!("2020-10-20")));
        put(&store, "2020-10-21", edit, Some(&edited()));

        let set = EventSet::build(&store, Unknowns::Fail).unwrap();

        assert_eq!(licensee_of(&set, 54960).as_deref(), Some(expected), "original fetched {original}");
        assert_eq!(set.archive.events, 6);
    }
}

#[test]
fn event_set_breaks_a_tie_in_fetch_time_by_the_later_page() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();
    put(&store, "2020-10-21", LATER, Some(&edited()));
    put(&store, "2020-10-20", LATER, Some(fixture!("2020-10-20")));

    let set = EventSet::build(&store, Unknowns::Fail).unwrap();

    assert_eq!(licensee_of(&set, 54960).as_deref(), Some("Hillis-Carnes Engineering"));
}

#[test]
fn event_set_reads_only_the_newest_capture_of_a_date() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();
    put(&store, "2020-10-20", EARLIER, Some(&edited()));
    put(&store, "2020-10-20", LATER, Some(fixture!("2020-10-20")));

    let set = EventSet::build(&store, Unknowns::Fail).unwrap();

    assert_eq!(licensee_of(&set, 54960).as_deref(), Some("Hillis-Carnes Engineering Associates, Inc."));
    assert_eq!(set.archive.pages, 1);
}

#[test]
fn event_set_fails_on_a_broken_page_unless_quarantining() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();
    put(&store, "2020-10-20", EARLIER, Some(fixture!("2020-10-20")));
    put(&store, "2020-10-22", EARLIER, Some(b"<html><main>not an event page</main></html>"));

    let error = EventSet::build(&store, Unknowns::Fail).unwrap_err();
    let quarantined = EventSet::build(&store, Unknowns::Quarantine).unwrap();

    assert!(format!("{error:#}").contains("the event page for 2020-10-22"), "{error:#}");
    assert_eq!(quarantined.archive.failed, [NaiveDate::from_ymd_opt(2020, 10, 22).unwrap()]);
    assert_eq!((quarantined.archive.pages, quarantined.archive.events), (2, 6));
}
