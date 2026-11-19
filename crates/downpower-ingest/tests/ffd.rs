//! Fitness-for-duty identification (PRD-0005 uat-001) over the events on the fixture page for
//! 2009-08-24, recoded: an event is FFD by its 10 CFR section or the reviewed lists, never by its
//! title, and a retracted event is none.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use pretty_assertions::assert_eq;

use downpower_core::ffd::{Kind, Role, Site, Substance, Tags, Test};
use downpower_ingest::cannabis::Laws;
use downpower_ingest::events::{self, Archive, Category, Event, EventSet, Section};
use downpower_ingest::facilities::Identity;
use downpower_ingest::ffd::{self, Collector, Ffd, Why};
use downpower_ingest::ledger::{Builder, Catalog, Unknowns};
use downpower_ingest::status::{Detail, Row, StatusPage};
use downpower_ingest::units::Docket;

/// A catalog's phrase rules, none of them.
const NO_RULES: &str = "strip = []\nsubstance = []\nrole = []\nkind = []\ntest = []";

/// The fixture page's events, by number.
fn fixture() -> Vec<Event> {
    events::parse(include_bytes!("fixtures/events/2009-08-24.html")).unwrap()
}

fn coded(event: &Event, code: &str) -> Event {
    Event {
        sections: vec![Section {
            code: Some(code.to_owned()),
            description: "FITNESS FOR DUTY".to_owned(),
        }],
        ..event.clone()
    }
}

#[test]
fn uat_ffd_identify_takes_the_three_sections_and_their_paragraphs() {
    assert_eq!(ffd::section("26.73"), Some("26.73"));
    assert_eq!(ffd::section("26.719 (b)(2)"), Some("26.719"));
    assert_eq!(ffd::section("26.719(c)(1)"), Some("26.719"));
    assert_eq!(ffd::section("26.417(b)(1)"), Some("26.417(b)(1)"));
    assert_eq!(ffd::section("26.417(b)(2)"), None, "another paragraph of 26.417");
    assert_eq!(ffd::section("26.7190"), None);
    assert_eq!(ffd::section("26.7"), None);
    assert_eq!(ffd::section("73.71(b)(1)"), None);
}

#[test]
fn uat_ffd_identify_by_section_and_the_reviewed_lists() {
    let events = fixture();
    let rules = Ffd::parse(&format!(
        "include = [{{ number = {}, why = \"reports a positive test under 73.71\" }}]\nexclude = [{{ number = {}, why = \"coded 26.719 by mistake\" }}]\n{NO_RULES}",
        events[1].number, events[2].number
    ))
    .unwrap();
    assert_eq!(rules.identify(&coded(&events[0], "26.719(b)(1)")), Some(Why::Section("26.719".to_owned())));
    assert_eq!(rules.identify(&coded(&events[0], "26.73")), Some(Why::Section("26.73".to_owned())));
    assert_eq!(rules.identify(&events[0]), None, "the event as published carries no FFD section");
    assert_eq!(rules.identify(&events[1]), Some(Why::Listed));
    assert_eq!(rules.identify(&coded(&events[2], "26.719")), None, "excluded");
    let retracted = Event {
        retracted: true,
        ..coded(&events[0], "26.719")
    };
    assert_eq!(rules.identify(&retracted), None);
    // A title alone makes nothing an FFD event.
    let titled = Event {
        title: Some("FITNESS FOR DUTY REPORT".to_owned()),
        ..events[0].clone()
    };
    assert_eq!(rules.identify(&titled), None);
}

#[test]
fn uat_ffd_identify_rejects_a_bad_catalog() {
    assert!(Ffd::parse(&format!("include = [{{ number = 1, why = \"\" }}]\nexclude = []\n{NO_RULES}")).is_err(), "no reason");
    assert!(
        Ffd::parse(&format!("include = [{{ number = 1, why = \"a\" }}]\nexclude = [{{ number = 1, why = \"b\" }}]\n{NO_RULES}")).is_err(),
        "listed twice"
    );
    assert!(
        Ffd::parse("include = []\nexclude = []\nstrip = []\nsubstance = [{ value = \"alcohol\", pattern = \"(\" }]\nrole = []\nkind = []\ntest = []").is_err(),
        "a bad pattern"
    );
    Ffd::catalog().unwrap();
}

/// The tags of a fixture event rewritten to carry `text` alone.
fn tags_of(text: &str) -> Tags {
    let event = Event {
        title: None,
        text: text.to_owned(),
        ..fixture()[0].clone()
    };
    Ffd::catalog().unwrap().tags(&event)
}

#[test]
fn uat_ffd_tags_read_substance_role_kind_and_test() {
    assert_eq!(
        tags_of("A contract supervisor had a confirmed positive for marijuana during a random fitness-for-duty test."),
        Tags {
            substance: Substance::Marijuana,
            role: Role::Supervisor,
            kind: Kind::Positive,
            test: Test::Random,
        }
    );
    // The boilerplate names a test, not a finding.
    assert_eq!(
        tags_of("A LICENSED OPERATOR REFUSED TO TAKE A FOR-CAUSE DRUG AND ALCOHOL TEST."),
        Tags {
            substance: Substance::Unnamed,
            role: Role::LicensedOperator,
            kind: Kind::Refusal,
            test: Test::ForCause,
        }
    );
    assert_eq!(
        tags_of("An employee tested positive for alcohol and cocaine during a follow-up test.").substance,
        Substance::Cocaine,
        "a drug outranks alcohol"
    );
    assert_eq!(tags_of("A can of beer was found inside the protected area.").kind, Kind::SubstanceFound);
    assert_eq!(
        tags_of("The HHS-certified laboratory reported a false negative on a blind performance sample.").kind,
        Kind::ProgramFailure
    );
    assert_eq!(
        tags_of("A contractor attempted to subvert the test with synthetic urine during pre-access testing.").kind,
        Kind::Subversion
    );
    assert_eq!(
        tags_of("A contractor attempted to subvert the test with synthetic urine during pre-access testing.").test,
        Test::PreAccess
    );
}

#[test]
fn uat_ffd_tags_skip_the_known_misfires() {
    // THC as a typo of "the" in an all-caps page.
    assert_eq!(tags_of("THC LICENSEE NOTIFIED THC NRC RESIDENT INSPECTOR.").substance, Substance::Unnamed);
    // A tritium sample's "positive".
    assert_eq!(tags_of("Tritium sample results were positive at the monitoring well.").kind, Kind::Unnamed);
    // Isopropyl alcohol and the ATF.
    assert_eq!(tags_of("Isopropyl alcohol was spilled near the drum.").substance, Substance::Unnamed);
    assert_eq!(tags_of("The Bureau of Alcohol, Tobacco, Firearms and Explosives was notified.").substance, Substance::Unnamed);
    // Weapons "contraband".
    assert_eq!(tags_of("Security confiscated contraband weapons at the vehicle checkpoint.").kind, Kind::Unnamed);
}

/// The fixture page's events recoded: EN 45290 (Browns Ferry 3, AL) a licensed operator's random
/// marijuana positive; EN 45286 (Crystal River 3) moved to Vogtle 3 before its commercial operation;
/// EN 45289 moved to Summer 2 and 3, which never operated; EN 45285 a fuel cycle event; EN 45287
/// coded FFD but retracted.
fn ffd_events(catalog: &Catalog) -> EventSet {
    let ffd = |event: &mut Event, text: &str| {
        event.sections = vec![Section {
            code: Some("26.719(b)(1)".to_owned()),
            description: "FITNESS FOR DUTY".to_owned(),
        }];
        text.clone_into(&mut event.text);
    };
    let mut events: BTreeMap<u32, Event> = fixture().into_iter().map(|event| (event.number, event)).collect();
    ffd(
        events.get_mut(&45290).unwrap(),
        "A licensed operator tested positive for marijuana during a random fitness-for-duty test.",
    );
    ffd(events.get_mut(&45286).unwrap(), "A contractor tested positive for alcohol during pre-access testing.");
    ffd(events.get_mut(&45289).unwrap(), "A craft supervisor refused a for-cause test.");
    ffd(events.get_mut(&45285).unwrap(), "An employee tested positive for cocaine.");
    ffd(events.get_mut(&45287).unwrap(), "A supervisor tested positive for alcohol.");
    events.get_mut(&45287).unwrap().retracted = true;
    let mut identities = catalog.facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap();
    let docket = |text: &str| Docket::try_from(text.to_owned()).unwrap();
    identities.insert(45286, Identity::Reactor { dockets: vec![docket("05200025")] });
    identities.insert(
        45289,
        Identity::Reactor {
            dockets: vec![docket("05200027"), docket("05200028")],
        },
    );
    // Recategorized once identified: the facility catalog matches a fuel cycle name on its state.
    events.get_mut(&45285).unwrap().category = Category::FuelCycleFacility;
    EventSet {
        events,
        identities,
        pages: BTreeMap::new(),
        archive: Archive::default(),
    }
}

fn status_page(units: &[&str]) -> StatusPage {
    StatusPage::Report(
        units
            .iter()
            .map(|unit| Row {
                region: 2,
                unit: (*unit).to_owned(),
                power: 100,
                detail: Some(Detail {
                    down: None,
                    reason: None,
                    changed: false,
                    scrams: 0,
                }),
            })
            .collect(),
    )
}

#[test]
fn uat_ffd_series_collects_events_by_site_and_exposure_by_state() {
    let catalog = Catalog::embedded().unwrap();
    let events = ffd_events(&catalog);
    let laws = Laws::catalog().unwrap();
    let mut builder = Builder::new(&catalog, None, &events, Unknowns::Fail);
    let mut collector = Collector::new(&events, &catalog.units, &Ffd::catalog().unwrap(), &laws);
    for date in ["2009-08-23", "2009-08-24"] {
        collector.push(&builder.day(date.parse().unwrap(), Some(&status_page(&["Browns Ferry 3", "Fermi 2"]))).unwrap());
    }
    let series = collector.finish();

    let sites: Vec<(u32, Site, Option<&str>)> = series.events.iter().map(|event| (event.number, event.site, event.state.as_deref())).collect();
    assert_eq!(
        sites,
        [
            (45285, Site::Facility, events.events[&45285].state.as_deref()),
            (45286, Site::Construction, events.events[&45286].state.as_deref()),
            (45289, Site::Construction, events.events[&45289].state.as_deref()),
            (45290, Site::Operating, Some("AL")),
        ],
        "the retracted event counts toward nothing"
    );
    let browns_ferry = series.events.iter().find(|event| event.number == 45290).unwrap();
    assert_eq!(
        (browns_ferry.tags.substance, browns_ferry.tags.role, browns_ferry.tags.test),
        (Substance::Marijuana, Role::LicensedOperator, Test::Random)
    );

    let exposure: Vec<(&str, i32, u32, u32, u32)> = series
        .exposure
        .iter()
        .map(|row| (row.state.as_str(), row.year, row.unit_days, row.adult_use_days, row.medical_days))
        .collect();
    // Michigan's comprehensive medical law took effect 2008-12-04; Alabama had none in 2009.
    assert_eq!(exposure, [("AL", 2009, 2, 0, 0), ("MI", 2009, 2, 0, 2)]);
    assert_eq!((series.first, series.last), (Some("2009-08-23".parse().unwrap()), Some("2009-08-24".parse().unwrap())));
    assert_eq!(series.laws.len(), 51);
}
