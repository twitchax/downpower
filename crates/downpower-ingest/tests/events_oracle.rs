//! The Event Notification parser against the NRC's own raw data file (PRD-0003 uat-002). The
//! fixtures are `event-notification-rpt-lastmonth.txt` byte for byte as nrc.gov serves it (last
//! modified 2025-08-05, its columns in docs/research/data-sources.md) and its 54 events cut from
//! their newest stored pages by `cargo xtask oracle`. Pages are rendered from the NRC's current
//! database, so an event whose page carries a later Last Update Date than the file was edited
//! since: its differences are reported, not failed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::fmt::Debug;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use pretty_assertions::assert_eq;

use downpower_ingest::events::{self, Category, Event, Scram, Section, UnitInfo};

const FILE: &[u8] = include_bytes!("fixtures/events/oracle/event-notification-rpt-lastmonth.txt");
const PAGES: &[u8] = include_bytes!("fixtures/events/oracle/events.html");

/// The events whose page carries a later Last Update Date than the file.
const EDITED: [u32; 7] = [57243, 57746, 57788, 57796, 57818, 57826, 57827];

/// One row of the file, by column name.
type Row<'a> = HashMap<&'a str, &'a str>;

/// The file's rows: CRLF lines of `|`-separated cells under a header of column names, each line
/// ending in a `|` and the file in a blank line. It is ASCII, and a lone NUL is the NRC's null,
/// written where a column has no value (a County Name, a spare unit slot's Scram Code): it reads as
/// empty.
fn rows() -> Vec<Row<'static>> {
    assert!(FILE.is_ascii());
    let file = std::str::from_utf8(FILE).unwrap();
    let mut lines = file.strip_suffix("\r\n\r\n").unwrap().split("\r\n");
    let header: Vec<&str> = lines.next().unwrap().split('|').collect();
    lines
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(|cell| if cell == "\0" { "" } else { cell }).collect();
            assert_eq!(cells.len(), header.len(), "{line:.40}");
            assert!(cells.iter().all(|cell| !cell.contains('\0')), "{line:.40}");
            header.iter().copied().zip(cells).collect()
        })
        .collect()
}

/// A cell as written.
fn raw<'a>(row: &Row<'a>, column: &str) -> &'a str {
    row.get(column).unwrap_or_else(|| panic!("no column {column:?}"))
}

fn cell<'a>(row: &Row<'a>, column: &str) -> &'a str {
    raw(row, column).trim()
}

fn number(row: &Row<'_>) -> u32 {
    cell(row, "En No").parse().unwrap()
}

fn date(row: &Row<'_>, column: &str) -> NaiveDate {
    NaiveDate::parse_from_str(cell(row, column), "%m/%d/%Y").unwrap()
}

/// `9:47:00`: the page's `09:47` with seconds, which are always zero.
fn time(row: &Row<'_>, column: &str) -> NaiveTime {
    NaiveTime::parse_from_str(cell(row, column), "%H:%M:%S").unwrap()
}

/// `7/2/2025 4:42:00 PM`, of which the page shows the date.
fn updated(row: &Row<'_>) -> NaiveDate {
    NaiveDateTime::parse_from_str(cell(row, "Last Updated Dt"), "%m/%d/%Y %I:%M:%S %p").unwrap().date()
}

/// `Cfr Cd1..4` and `Cfr Descr1..4`. The page shows a section whose code is a space (an Agreement
/// State event's) by its description alone, and leaves out one with no code at all, as EN 57824's
/// `Non-Power Reactor Event`: 65 stored non-power reactor events since 2020 show no section.
fn sections(row: &Row<'_>) -> Vec<Section> {
    (1..=4)
        .filter_map(|slot| {
            let code = raw(row, &format!("Cfr Cd{slot}"));
            (!code.is_empty()).then(|| Section {
                code: Some(code.trim()).filter(|code| !code.is_empty()).map(str::to_owned),
                description: cell(row, &format!("Cfr Descr{slot}")).to_owned(),
            })
        })
        .collect()
}

/// The unit slots with a unit in `Unit Ind1..3`, which the page's table lists. The rest hold
/// placeholders: a non-reactor event's slots read `N` at 0% in Power Operation, and a reactor
/// event's spare slots have a blank or null scram code.
fn units(row: &Row<'_>) -> Vec<UnitInfo> {
    // The file's column names are inconsistently spaced.
    const COLUMNS: [[&str; 6]; 3] = [
        ["Scram Code 1", "RX CRIT 1", "Initial PWR 1", "Initial RX Mode1", "Current PWR 1", "Current RX Mode 1"],
        ["Scram Code 2", "RX CRIT 2", "Initial PWR 2", "Initial RX Mode 2", "Current PWR 2", "Current RX Mode 2"],
        ["Scram Code 3", "RX CRIT 3", "Initial PWR 3", "Initial RX Mode 3", "Current PWR 3", "Current RX Mode 3"],
    ];
    let mode = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    (1..=3)
        .zip(COLUMNS)
        .filter_map(|(slot, [code, critical, initial_power, initial_mode, current_power, current_mode])| {
            let unit = cell(row, &format!("Unit Ind{slot}"));
            (!unit.is_empty()).then(|| UnitInfo {
                unit: unit.parse().unwrap(),
                scram: scram(cell(row, code)),
                critical: match cell(row, critical) {
                    "Y" => true,
                    "N" => false,
                    other => panic!("RX CRIT {other:?}"),
                },
                initial_power: cell(row, initial_power).parse().unwrap(),
                initial_mode: mode(cell(row, initial_mode)),
                current_power: cell(row, current_power).parse().unwrap(),
                current_mode: mode(cell(row, current_mode)),
            })
        })
        .collect()
}

/// The data dictionary's scram codes, read independently of the parser.
fn scram(code: &str) -> Option<Scram> {
    Some(match code {
        "" => return None,
        "N" => Scram::No,
        "A/R" => Scram::AutoReactor,
        "M/R" => Scram::ManualReactor,
        "A" => Scram::Auto,
        "M" => Scram::Manual,
        other => panic!("scram code {other:?}"),
    })
}

/// A field where the file and the page disagree: its name, the file's value and the page's.
type Difference = (&'static str, String, String);

fn compare<T: PartialEq + Debug>(found: &mut Vec<Difference>, field: &'static str, file: &T, page: &T) {
    if file != page {
        found.push((field, format!("{file:?}"), format!("{page:?}")));
    }
}

/// Every field the file and `event` share, compared in the parser's types.
fn differences(row: &Row<'_>, event: &Event) -> Vec<Difference> {
    let mut found = Vec::new();
    let category = Category::ALL.into_iter().find(|category| category.label() == cell(row, "Event Desc"));
    compare(&mut found, "category", &category, &Some(event.category));
    compare(&mut found, "notification date", &date(row, "Notification Dt"), &event.notified.date);
    compare(&mut found, "notification time", &time(row, "Notification Time"), &event.notified.time);
    compare(&mut found, "event date", &date(row, "Event Dt"), &event.occurred.date);
    compare(&mut found, "event time", &time(row, "Event Time"), &event.occurred.time);
    compare(&mut found, "event time zone", &Some(cell(row, "Time Zone")), &event.occurred.zone.as_deref());
    compare(&mut found, "last update date", &Some(updated(row)), &event.updated);
    compare(&mut found, "10 CFR sections", &sections(row), &event.sections);
    compare(&mut found, "units", &units(row), &event.units);
    found
}

#[test]
fn uat_events_oracle_cuts_hold_the_files_54_events_in_its_order() {
    let numbers: Vec<u32> = rows().iter().map(number).collect();
    let parsed: Vec<u32> = events::parse(PAGES).unwrap().iter().map(|event| event.number).collect();

    assert_eq!(numbers.len(), 54);
    assert_eq!(parsed, numbers);
}

#[test]
fn uat_events_oracle_pages_match_the_files_columns_but_for_later_edits() {
    let events: HashMap<u32, Event> = events::parse(PAGES).unwrap().into_iter().map(|event| (event.number, event)).collect();

    let (mut edited, mut mismatches) = (Vec::new(), Vec::new());
    for row in rows() {
        let event = &events[&number(&row)];
        let found = differences(&row, event);
        if event.updated > Some(updated(&row)) {
            edited.push(event.number);
            for (field, file, page) in found {
                eprintln!("EN {} was edited after the file: its {field} reads {file} in the file and {page} on the page", event.number);
            }
        } else {
            mismatches.extend(found.into_iter().map(|(field, file, page)| format!("EN {}: {field}: file {file}, page {page}", event.number)));
        }
    }

    assert_eq!(mismatches, Vec::<String>::new());
    assert_eq!(edited, EDITED);
}
