//! Scram episodes (PRD-0007 uat-004), through the ledger builder, over nine status pages from
//! 2009-08-22 and the events on the fixture page for 2009-08-24, recoded per test: Crystal River 3
//! (EN 45286), Browns Ferry 3 (EN 45290), and Peach Bottom 2 and 3 (EN 45289).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;

use downpower_core::trends::ScramKind;
use downpower_core::triggers::{End, Episode, Scram, Series, Tags, UnitExposure};
use downpower_ingest::episodes::Collector;
use downpower_ingest::events::{self, Archive, EventSet, Scram as Code};
use downpower_ingest::ledger::{Builder, Catalog, Unknowns};
use downpower_ingest::status::{Detail, Row, StatusPage};

const DAYS: usize = 9;

const CR3: &str = "Crystal River 3";
const BF3: &str = "Browns Ferry 3";
const PB2: &str = "Peach Bottom 2";
const PB3: &str = "Peach Bottom 3";

const CR3_DOCKET: &str = "05000302";
const BF3_DOCKET: &str = "05000296";
const PB2_DOCKET: &str = "05000277";
const PB3_DOCKET: &str = "05000278";

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

/// A unit's power and reason on one page; `None` leaves the unit off it.
type Line = Option<(u8, Option<&'static str>)>;

const REFUELING: Line = Some((0, Some("Refueling Outage")));

/// The fixture page's events, identified with the committed catalogs, after `edit`.
fn events(catalog: &Catalog, edit: impl FnOnce(&mut EventSet)) -> EventSet {
    let events: BTreeMap<u32, _> = events::parse(include_bytes!("fixtures/events/2009-08-24.html"))
        .unwrap()
        .into_iter()
        .map(|event| (event.number, event))
        .collect();
    let identities = catalog.facilities.identify_all(&events, Unknowns::Fail, &mut BTreeMap::new()).unwrap();
    let mut set = EventSet {
        events,
        identities,
        pages: BTreeMap::new(),
        archive: Archive::default(),
    };
    edit(&mut set);
    set
}

/// Sets unit `index` of event `number` to scram with `code` from `power` on `date`.
fn scram(set: &mut EventSet, number: u32, index: usize, code: Code, power: u8, date: &str) {
    let event = set.events.get_mut(&number).unwrap();
    event.occurred.date = day(date);
    event.units[index].scram = Some(code);
    event.units[index].initial_power = power;
}

/// The series over the pages from 2009-08-22 that `lines` give, every row at 1 MW.
fn series(edit: impl FnOnce(&mut EventSet), lines: &[(&str, [Line; DAYS])]) -> Series {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, edit);
    let mut builder = Builder::new(&catalog, None, &events, Unknowns::Fail);
    let mut collector = Collector::new(&events, &catalog.units, |_| Tags::default());
    for (index, date) in day("2009-08-22").iter_days().take(DAYS).enumerate() {
        let rows = lines
            .iter()
            .filter_map(|(unit, days)| {
                let (power, reason) = days[index]?;
                Some(Row {
                    region: 2,
                    unit: (*unit).to_owned(),
                    power,
                    detail: Some(Detail {
                        down: None,
                        reason: reason.map(str::to_owned),
                        changed: false,
                        scrams: 0,
                    }),
                })
            })
            .collect();
        let mut days = builder.day(date, Some(&StatusPage::Report(rows))).unwrap();
        for row in &mut days {
            row.capacity_kw = Some(1_000);
        }
        collector.push(&days);
    }
    collector.finish()
}

/// The scrams of [`series`].
fn collect(edit: impl FnOnce(&mut EventSet), lines: &[(&str, [Line; DAYS])]) -> Vec<Scram> {
    series(edit, lines).scrams
}

/// `points` percentage points of 1 MW for a day, in kWh.
const fn kwh(points: u64) -> u64 {
    points * 1_000 * 24 / 100
}

fn unit_scram(number: u32, docket: &str, date: &str, initial_power: u8, episode: Option<Episode>) -> Scram {
    Scram {
        number,
        docket: docket.to_owned(),
        date: day(date),
        kind: ScramKind::Automatic,
        initial_power,
        tags: Tags::default(),
        episode,
    }
}

#[test]
fn uat_scram_episodes_run_from_the_drop_until_the_unit_is_back_or_refueling() {
    let scrams = collect(
        |set| {
            scram(set, 45286, 0, Code::AutoReactor, 100, "2009-08-24");
            scram(set, 45290, 0, Code::AutoReactor, 100, "2009-08-24");
            scram(set, 45289, 0, Code::AutoReactor, 5, "2009-08-24");
            scram(set, 45289, 1, Code::AutoReactor, 100, "2009-08-24");
        },
        &[
            // The page on the 24th predates the scram; 92% is back within 10 points of 100%.
            (
                CR3,
                [
                    Some((100, None)),
                    Some((100, None)),
                    Some((100, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((30, None)),
                    Some((85, None)),
                    Some((92, None)),
                    Some((100, None)),
                ],
            ),
            // Down the day of the event, then into refueling.
            (
                BF3,
                [
                    Some((100, None)),
                    Some((100, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((0, None)),
                    REFUELING,
                    REFUELING,
                    REFUELING,
                    REFUELING,
                ],
            ),
            // From 5%: no drop can show.
            (PB2, [Some((5, None)); DAYS]),
            // The reports never show the drop.
            (PB3, [Some((100, None)); DAYS]),
        ],
    );
    assert_eq!(
        scrams,
        [
            unit_scram(
                45286,
                CR3_DOCKET,
                "2009-08-24",
                100,
                Some(Episode {
                    start: day("2009-08-25"),
                    days: 4,
                    lost_kwh: kwh(100 + 100 + 70 + 15),
                    end: End::Returned,
                })
            ),
            unit_scram(45289, PB2_DOCKET, "2009-08-24", 5, None),
            unit_scram(45289, PB3_DOCKET, "2009-08-24", 100, None),
            unit_scram(
                45290,
                BF3_DOCKET,
                "2009-08-24",
                100,
                Some(Episode {
                    start: day("2009-08-24"),
                    days: 3,
                    lost_kwh: kwh(300),
                    end: End::Refueling,
                })
            ),
        ]
    );
}

#[test]
fn uat_scram_episodes_end_at_the_next_scram_or_stay_open_and_skip_retracted_events() {
    let scrams = collect(
        |set| {
            scram(set, 45286, 0, Code::AutoReactor, 100, "2009-08-23");
            // A second scram on Crystal River 3, from 30% during the restart.
            let mut second = set.events[&45286].clone();
            second.number = 45999;
            second.occurred.date = day("2009-08-27");
            second.units[0].initial_power = 30;
            set.events.insert(45999, second);
            let identity = set.identities[&45286].clone();
            set.identities.insert(45999, identity);
            scram(set, 45290, 0, Code::AutoReactor, 100, "2009-08-23");
            set.events.get_mut(&45290).unwrap().retracted = true;
            scram(set, 45289, 1, Code::AutoReactor, 100, "2009-08-23");
        },
        &[
            (
                CR3,
                [
                    Some((100, None)),
                    Some((100, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((20, None)),
                    Some((30, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((0, None)),
                ],
            ),
            (
                BF3,
                [
                    Some((100, None)),
                    Some((100, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((0, None)),
                    Some((100, None)),
                    Some((100, None)),
                    Some((100, None)),
                ],
            ),
            // Off the page the day after it went down: the rows stop showing it.
            (
                PB3,
                [
                    Some((100, None)),
                    Some((100, None)),
                    Some((0, None)),
                    None,
                    Some((0, None)),
                    Some((0, None)),
                    Some((100, None)),
                    Some((100, None)),
                    Some((100, None)),
                ],
            ),
        ],
    );
    assert_eq!(
        scrams,
        [
            unit_scram(
                45286,
                CR3_DOCKET,
                "2009-08-23",
                100,
                Some(Episode {
                    start: day("2009-08-24"),
                    days: 3,
                    lost_kwh: kwh(100 + 100 + 80),
                    end: End::NextScram,
                })
            ),
            unit_scram(
                45289,
                PB3_DOCKET,
                "2009-08-23",
                100,
                Some(Episode {
                    start: day("2009-08-24"),
                    days: 1,
                    lost_kwh: kwh(100),
                    end: End::Open,
                })
            ),
            unit_scram(
                45999,
                CR3_DOCKET,
                "2009-08-27",
                30,
                Some(Episode {
                    start: day("2009-08-28"),
                    days: 3,
                    lost_kwh: kwh(30 * 3),
                    end: End::Open,
                })
            ),
        ]
    );
}

#[test]
fn uat_scram_episodes_take_the_kind_and_the_tags_from_the_event() {
    let catalog = Catalog::embedded().unwrap();
    let events = events(&catalog, |set| scram(set, 45290, 0, Code::ManualReactor, 100, "2009-08-24"));
    let tags = Tags {
        system: downpower_core::triggers::System::Feedwater,
        circumstance: downpower_core::triggers::Circumstance::Testing,
    };
    let collector = Collector::new(&events, &catalog.units, |event| if event.number == 45290 { tags } else { Tags::default() });
    let scrams = collector.finish().scrams;
    let browns_ferry = scrams.iter().find(|scram| scram.number == 45290).unwrap();
    assert_eq!((browns_ferry.kind, browns_ferry.tags, browns_ferry.episode), (ScramKind::Manual, tags, None));
}

#[test]
fn uat_scram_episodes_series_carries_each_units_days_by_month() {
    let lines = [
        (CR3, [Some((100, None)); DAYS]),
        // Off one page.
        (
            PB3,
            [
                Some((100, None)),
                Some((100, None)),
                Some((100, None)),
                None,
                Some((100, None)),
                Some((100, None)),
                Some((100, None)),
                Some((100, None)),
                Some((100, None)),
            ],
        ),
    ];
    let series = series(|_| {}, &lines);
    assert_eq!((series.first, series.last), (Some(day("2009-08-01")), Some(day("2009-08-30"))));
    assert_eq!(
        series.units,
        [
            UnitExposure {
                docket: PB3_DOCKET.to_owned(),
                name: PB3.to_owned(),
                days: vec![8],
            },
            UnitExposure {
                docket: CR3_DOCKET.to_owned(),
                name: CR3.to_owned(),
                days: vec![9],
            },
        ],
        "by docket"
    );
}
