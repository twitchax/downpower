//! The Event Notification Report parser. Every page since 1999 renders through one Drupal template:
//! a summary listing the page's event numbers, then per event a `<div class="grid border"
//! id="enNNNNN">` of `<b>Label:</b> value` lines, the Power Reactor Unit Info table for reactor
//! events, and the event text. Non-reactor categories use a second set of header labels (a
//! licensee's rather than a facility's). Any other label, order or element fails the parse, so a
//! template change stops the backfill instead of quietly dropping fields.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::iter::Peekable;
use std::ops::Range;

use anyhow::{Context as _, anyhow, bail, ensure};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use regex::{Captures, Regex};
use scraper::{ElementRef, Html, Node, Selector};
use serde::{Deserialize, Serialize};

use crate::ledger::Unknowns;
use crate::raw::{RawStore, Series};
use crate::{Res, Void, nrc};

/// The NRC's event categories, as the page labels them. Two were renamed: Research Reactor became
/// Non-Power Reactor, and General Information became General Information or Other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    PowerReactor,
    FuelCycleFacility,
    AgreementState,
    NonAgreementState,
    GeneralInformationOrOther,
    GeneralInformation,
    OtherNuclearMaterial,
    Part21,
    Hospital,
    NonPowerReactor,
    ResearchReactor,
    SpentFuelStorage,
    Transportation,
}

impl Category {
    /// Every category, in the order above.
    pub const ALL: [Self; 13] = [
        Self::PowerReactor,
        Self::FuelCycleFacility,
        Self::AgreementState,
        Self::NonAgreementState,
        Self::GeneralInformationOrOther,
        Self::GeneralInformation,
        Self::OtherNuclearMaterial,
        Self::Part21,
        Self::Hospital,
        Self::NonPowerReactor,
        Self::ResearchReactor,
        Self::SpentFuelStorage,
        Self::Transportation,
    ];

    /// The label the page gives the category.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PowerReactor => "Power Reactor",
            Self::FuelCycleFacility => "Fuel Cycle Facility",
            Self::AgreementState => "Agreement State",
            Self::NonAgreementState => "Non-Agreement State",
            Self::GeneralInformationOrOther => "General Information or Other",
            Self::GeneralInformation => "General Information",
            Self::OtherNuclearMaterial => "Other Nuclear Material",
            Self::Part21 => "Part 21",
            Self::Hospital => "Hospital",
            Self::NonPowerReactor => "Non-Power Reactor",
            Self::ResearchReactor => "Research Reactor",
            Self::SpentFuelStorage => "Independent Spent Fuel Storage Installation",
            Self::Transportation => "Transportation Event",
        }
    }
}

/// One event as its page shows it when fetched. Pages are re-rendered from the NRC's database, so
/// an old page carries every update made since (DESIGN.md Decision 30).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Event {
    pub number: u32,
    pub category: Category,
    /// The page's "THIS EVENT HAS BEEN RETRACTED" banner: the NRC's own flag, and the one that
    /// counts (Decision 30). A retraction in the timeline is not enough on its own, since many
    /// were later withdrawn (PRD-0003.log.md#t-002).
    pub retracted: bool,
    pub site: Site,
    /// The NRC region, 1 to 4, or 0 for events headquarters handles (non-power reactors, for one).
    pub region: u8,
    pub state: Option<String>,
    /// Who called the event in.
    pub notified_by: Option<String>,
    /// The NRC headquarters operations officer who took the call.
    pub ops_officer: Option<String>,
    pub notified: Stamp,
    pub occurred: Stamp,
    /// The Last Update Date.
    pub updated: NaiveDate,
    /// `None` when the page leaves it blank.
    pub class: Option<EmergencyClass>,
    pub sections: Vec<Section>,
    /// The Person (Organization) list as written, e.g. `Ruiz, Robert (R3 DRPBC)`.
    pub persons: Vec<String>,
    /// The Power Reactor Unit Info table; empty for every other category.
    pub units: Vec<UnitInfo>,
    /// The text's first line, which is the event's headline on all but a handful of early pages.
    pub title: Option<String>,
    /// The event text with its line breaks, entities decoded and each line trimmed. Updates and
    /// retractions are appended to it as `* * * UPDATE ... * * *` paragraphs.
    pub text: String,
    /// The appended updates, in the text's order (Decision 30).
    pub updates: Vec<Update>,
    /// The date of an `EN Revision Imported Date` line, the NRC's 2020–2023 re-import. A note,
    /// not an update.
    pub revised: Option<NaiveDate>,
}

/// The header block, which takes one of two shapes by category.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    /// Power Reactor and Fuel Cycle Facility events.
    Facility(Facility),
    /// Every other category.
    Licensee(Licensee),
}

/// A power reactor site or a fuel cycle facility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Facility {
    /// The name as written: `VOGTLE` before 2020, `Vogtle` after, and `Vogtle 3/4` for the new units.
    pub name: String,
    /// The units the event involves, from `[1] [] [3]`.
    pub units: Vec<u8>,
    /// Each unit's reactor type (`[1] GE-3,[2] CE`), or a fuel cycle facility's process.
    pub rx_type: Option<String>,
    /// A fuel cycle facility's comments: an address or the process, one line per line.
    pub comments: Option<String>,
}

/// A materials licensee, a research reactor, a vendor reporting under Part 21, and so on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Licensee {
    /// The reporting organization: an Agreement State's program, or the licensee itself. The NRC
    /// cuts it at 35 characters.
    pub rep_org: Option<String>,
    pub name: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub license: Option<String>,
    /// Whether the licensee is in an Agreement State.
    pub agreement: bool,
    pub docket: Option<String>,
}

/// A date, a 24-hour time and the time zone as the page writes it (`ET`, `CST`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub zone: Option<String>,
}

/// The emergency classes of 10 CFR 50.72 and the emergency plans, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmergencyClass {
    NonEmergency,
    UnusualEvent,
    Alert,
    SiteAreaEmergency,
    GeneralEmergency,
}

/// One entry under "10 CFR Section", e.g. `50.72(b)(2)(iv)(B)` and `RPS ACTUATION - CRITICAL`.
/// Agreement State events carry a description alone (`AGREEMENT STATE`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Section {
    /// The section as written, spaces and all (`50.72(a) (1) (i)`, `PART 70 APP A (c)`).
    pub code: Option<String>,
    pub description: String,
}

/// One row of the Power Reactor Unit Info table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnitInfo {
    pub unit: u8,
    /// `None` when the cell is blank.
    pub scram: Option<Scram>,
    /// RX Crit: whether the reactor was critical.
    pub critical: bool,
    pub initial_power: u8,
    pub initial_mode: Option<String>,
    pub current_power: u8,
    pub current_mode: Option<String>,
}

/// The SCRAM Code column. The codes are kept as written: what separates `A` from `A/R` is not
/// documented, and several `A` events are actuations while shut down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scram {
    /// `N`: no scram.
    No,
    /// `A/R`: an automatic reactor scram.
    AutoReactor,
    /// `M/R`: a manual reactor scram.
    ManualReactor,
    /// `A`: an automatic actuation (16 units from 1999 to 2024).
    Auto,
    /// `M`: a manual scram (3 units).
    Manual,
}

/// One appended block of the event text: a marker line such as
/// `* * * UPDATE ON 09/28/2020 AT 1834 EDT FROM MATTHEW SENISON TO JEFFREY WHITED * * *` and the
/// lines after it. Markers are typed by hand, so any of the fields may be missing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Update {
    pub kind: UpdateKind,
    /// The marker line with its stars trimmed.
    pub marker: String,
    /// `None` when the marker has no date or a mistyped one (`09//22/99`, `O1/23/01`).
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub zone: Option<String>,
    /// Who called, as written.
    pub from: Option<String>,
    /// Who took the call, as written.
    pub to: Option<String>,
    /// The lines after the marker, up to the next one.
    pub text: String,
}

/// What a marker says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    /// `UPDATE`, `UPDATED`, and typos like `UDATE` and `UPDATEON`.
    Update,
    /// `RETRACTION`, `RETRACTED`, `THIS EVENT WAS RETRACTED`, and so on.
    Retraction,
    /// `PARTIAL RETRACTION`: one reporting criterion withdrawn, the event standing.
    PartialRetraction,
    /// `UNRETRACTION`: an earlier retraction withdrawn.
    Unretraction,
    /// Any other marker: briefings, summaries, `HOO NOTE`s, closeouts.
    Note,
}

/// Every stored event, each from the copy fetched last (Decision 28), and what the archive held.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EventSet {
    pub events: BTreeMap<u32, Event>,
    pub archive: Archive,
}

/// What a build found in the stored event pages.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Archive {
    /// Pages read: the newest capture of each date that has one, failed pages included.
    pub pages: u64,
    /// Pages reading "No events found".
    pub empty: u64,
    /// Distinct events.
    pub events: u64,
    /// Dates whose page failed to parse and was skipped, under [`Unknowns::Quarantine`].
    pub failed: Vec<NaiveDate>,
}

impl EventSet {
    /// Parses the newest capture of every stored event page. An event on several pages comes from
    /// the page fetched last, and between pages fetched at the same moment, the later date: pages
    /// are rendered from the NRC's current database, so the newest fetch is the most current.
    ///
    /// # Errors
    ///
    /// When the raw store can't be read, or, under [`Unknowns::Fail`], a page fails to parse.
    pub fn build(store: &RawStore, unknowns: Unknowns) -> Res<Self> {
        let parser = Parser::new()?;
        let mut archive = Archive::default();
        // Each event's newest copy, under the fetch time and page date it came from.
        let mut newest: BTreeMap<u32, ((DateTime<Utc>, NaiveDate), Event)> = BTreeMap::new();
        for (date, record) in store.latest(Series::EventNotifications)? {
            let Some(hash) = &record.sha256 else { continue };
            archive.pages += 1;
            let events = match parser.parse(&store.get(hash)?).with_context(|| format!("the event page for {date}")) {
                Ok(events) => events,
                Err(error) if unknowns == Unknowns::Quarantine => {
                    tracing::warn!(%date, error = format!("{error:#}"), "skipping an event page that doesn't parse");
                    archive.failed.push(date);
                    continue;
                }
                Err(error) => return Err(error),
            };
            archive.empty += u64::from(events.is_empty());
            let copy = (record.fetched_at, date);
            for event in events {
                match newest.entry(event.number) {
                    Entry::Occupied(mut slot) if slot.get().0 < copy => {
                        slot.insert((copy, event));
                    }
                    Entry::Occupied(_) => {}
                    Entry::Vacant(slot) => {
                        slot.insert((copy, event));
                    }
                }
            }
        }
        let events: BTreeMap<u32, Event> = newest.into_iter().map(|(number, (_, event))| (number, event)).collect();
        archive.events = events.len() as u64;
        Ok(Self { events, archive })
    }
}

/// Parses one event page with a parser of its own; see [`Parser::parse`].
///
/// # Errors
///
/// As [`Parser::parse`].
pub fn parse(page: &[u8]) -> Res<Vec<Event>> {
    Parser::new()?.parse(page)
}

/// The event page parser, its selectors and marker patterns compiled once for a run of pages.
pub struct Parser {
    selectors: Selectors,
    markers: Markers,
}

impl Parser {
    /// # Errors
    ///
    /// When a selector or pattern fails to compile, which the tests rule out.
    pub fn new() -> Res<Self> {
        Ok(Self {
            selectors: Selectors::new()?,
            markers: Markers::new()?,
        })
    }

    /// Parses one event page, as stored in the raw store. A page that reads "No events found" has
    /// no events.
    ///
    /// # Errors
    ///
    /// When the page is not UTF-8 or departs from the template: an unknown category, label, label
    /// order or element, a unit table where none belongs (or none where one does), a value that
    /// doesn't parse, or a summary whose event numbers differ from the events below it.
    pub fn parse(&self, page: &[u8]) -> Res<Vec<Event>> {
        parse_page(self, page)
    }
}

fn parse_page(parser: &Parser, page: &[u8]) -> Res<Vec<Event>> {
    let text = std::str::from_utf8(page).context("an event page is not UTF-8")?;
    let html = Html::parse_document(nrc::main(text));
    let selectors = &parser.selectors;

    let mut blocks = html.select(&selectors.content);
    let (Some(content), None) = (blocks.next(), blocks.next()) else {
        bail!("expected exactly one {CONTENT}");
    };

    // Each event ends in a pair of <br>s, which carry nothing.
    let mut children = elements(content)?.into_iter().filter(|element| !is(*element, "br")).peekable();
    ensure!(children.next().is_some_and(|summary| is_class(summary, "div", "event-summary")), "expected the event summary first");
    let mut listed = match children.next() {
        Some(notice) if is_class(notice, "div", "text-align-center") && text_of(notice) == NO_EVENTS => {
            ensure!(children.next().is_none(), "events after {NO_EVENTS:?}");
            return Ok(Vec::new());
        }
        Some(numbers) if is_class(numbers, "p", "event-summary") => numbers
            .select(&selectors.anchor)
            .map(|anchor| {
                let number = text_of(anchor);
                number.parse::<u32>().with_context(|| format!("summary event number {number:?}"))
            })
            .collect::<Res<Vec<u32>>>()?,
        _ => bail!("neither events nor {NO_EVENTS:?}"),
    };

    let mut events = Vec::with_capacity(listed.len());
    while let Some(grid) = children.next() {
        let table = children.next_if(|element| is(*element, "table"));
        let heading = children.next();
        ensure!(
            heading.is_some_and(|b| is(b, "b") && text_of(b) == "Event Text"),
            "expected <b>Event Text</b> after {:?}",
            grid.value().id()
        );
        let body = children.next().filter(|div| is_class(*div, "div", "border")).context("expected the event text's div.border")?;
        events.push(event(grid, table, body, parser)?);
    }

    let mut held: Vec<u32> = events.iter().map(|event| event.number).collect();
    listed.sort_unstable();
    held.sort_unstable();
    ensure!(listed == held, "the summary lists {listed:?} but the page holds {held:?}");
    Ok(events)
}

const CONTENT: &str = "div.nrc-event-report-day";
const NO_EVENTS: &str = "No events found";
const RETRACTED: &str = "!!!!! THIS EVENT HAS BEEN RETRACTED !!!!!";
const SECTIONS: &str = "10 CFR Section:";
const UNIT_INFO: &str = "Power Reactor Unit Info";
const UNIT_HEADER: [&str; 7] = ["Unit", "SCRAM Code", "RX Crit", "Initial PWR", "Initial RX Mode", "Current PWR", "Current RX Mode"];

struct Selectors {
    content: Selector,
    anchor: Selector,
    caption: Selector,
    header: Selector,
    row: Selector,
    cell: Selector,
}

impl Selectors {
    fn new() -> Res<Self> {
        let parse = |css: &str| Selector::parse(css).map_err(|error| anyhow!("selector {css:?}: {error}"));
        Ok(Self {
            content: parse(CONTENT)?,
            anchor: parse("a")?,
            caption: parse("caption")?,
            header: parse("thead th")?,
            row: parse("tbody tr")?,
            cell: parse("td")?,
        })
    }
}

impl Category {
    fn from_label(label: &str) -> Res<Self> {
        Self::ALL.into_iter().find(|category| category.label() == label).with_context(|| format!("unknown category {label:?}"))
    }

    /// Power reactors and fuel cycle facilities get the facility header; the rest a licensee's.
    const fn has_facility(self) -> bool {
        matches!(self, Self::PowerReactor | Self::FuelCycleFacility)
    }
}

impl EmergencyClass {
    /// The class, written in upper case through 2019 and title case after.
    fn parse(written: &str) -> Res<Option<Self>> {
        Ok(Some(match written.to_ascii_uppercase().as_str() {
            "" => return Ok(None),
            "NON EMERGENCY" => Self::NonEmergency,
            "UNUSUAL EVENT" => Self::UnusualEvent,
            "ALERT" => Self::Alert,
            "SITE AREA EMERGENCY" => Self::SiteAreaEmergency,
            "GENERAL EMERGENCY" => Self::GeneralEmergency,
            _ => bail!("emergency class {written:?}"),
        }))
    }
}

impl Scram {
    fn parse(code: &str) -> Res<Option<Self>> {
        Ok(Some(match code {
            "" => return Ok(None),
            "N" => Self::No,
            "A/R" => Self::AutoReactor,
            "M/R" => Self::ManualReactor,
            "A" => Self::Auto,
            "M" => Self::Manual,
            _ => bail!("scram code {code:?}"),
        }))
    }
}

fn event(grid: ElementRef<'_>, table: Option<ElementRef<'_>>, body: ElementRef<'_>, parser: &Parser) -> Res<Event> {
    ensure!(is_class(grid, "div", "grid"), "expected an event's div.grid, found <{}>", grid.value().name());
    let id = grid.value().id().unwrap_or_default();
    let number: u32 = id.strip_prefix("en").and_then(|number| number.parse().ok()).with_context(|| format!("event id {id:?}"))?;
    parse_event(number, grid, table, body, parser).with_context(|| format!("event {number}"))
}

fn parse_event(number: u32, grid: ElementRef<'_>, table: Option<ElementRef<'_>>, body: ElementRef<'_>, parser: &Parser) -> Res<Event> {
    let mut blocks = elements(grid)?.into_iter().peekable();
    let mut retracted = false;
    while let Some(banner) = blocks.next_if(|div| is_class(*div, "div", "retracted")) {
        let banner = text_of(banner);
        ensure!(banner == RETRACTED, "retraction banner {banner:?}");
        retracted = true;
    }
    let (Some(category), Some(heading), Some(site), Some(dates), Some(class), Some(persons), None) =
        (blocks.next(), blocks.next(), blocks.next(), blocks.next(), blocks.next(), blocks.next(), blocks.next())
    else {
        bail!("expected two headings and four field blocks");
    };

    let category = Category::from_label(&heading_text(category)?)?;
    let heading = heading_text(heading)?;
    ensure!(heading == format!("Event Number: {number}"), "{heading} under id \"en{number}\"");

    let mut fields = Fields::of(site)?;
    let (site, region, state) = if category.has_facility() { facility(&mut fields)? } else { licensee(&mut fields)? };
    let notified_by = blank(fields.take("NRC Notified By")?);
    let ops_officer = blank(fields.take("HQ OPS Officer")?);
    fields.end()?;

    let mut fields = Fields::of(dates)?;
    let notified = stamp(&mut fields, "Notification")?;
    let occurred = stamp(&mut fields, "Event")?;
    let updated = date("Last Update Date", &fields.take("Last Update Date")?)?;
    fields.end()?;

    let mut fields = Fields::of(class)?;
    let (class, lines) = fields.take_with_lines("Emergency Class")?;
    let class = EmergencyClass::parse(&class)?;
    let mut lines = lines.into_iter();
    ensure!(lines.next().as_deref() == Some(SECTIONS), "expected {SECTIONS:?} after the emergency class");
    let sections = sections(&lines.collect::<Vec<_>>().join("\n"))?;
    fields.end()?;

    let mut fields = Fields::of(persons)?;
    let (head, persons) = fields.take_with_lines("Person (Organization)")?;
    ensure!(head.is_empty(), "Person (Organization) {head:?}");
    fields.end()?;

    let units = match (category, table) {
        (Category::PowerReactor, Some(table)) => unit_table(table, &parser.selectors)?,
        (Category::PowerReactor, None) => bail!("no {UNIT_INFO} table"),
        (_, None) => Vec::new(),
        (category, Some(_)) => bail!("a unit table on a {} event", category.label()),
    };

    let text = event_text(body)?;
    let revised = revised(&text)?;
    let timeline = parser.markers.updates(&text);
    Ok(Event {
        number,
        category,
        retracted,
        site,
        region,
        state,
        notified_by,
        ops_officer,
        notified,
        occurred,
        updated,
        class,
        sections,
        persons,
        units,
        title: title(&text),
        text,
        updates: timeline,
        revised,
    })
}

fn heading_text(heading: ElementRef<'_>) -> Res<String> {
    ensure!(is_class(heading, "div", "th"), "expected a div.th heading, found <{}>", heading.value().name());
    Ok(text_of(heading))
}

/// A facility's header lines, up to the notifier.
fn facility(fields: &mut Fields) -> Res<(Site, u8, Option<String>)> {
    let name = fields.take("Facility")?;
    ensure!(!name.is_empty(), "a facility with no name");
    let region = region(&fields.take("Region")?)?;
    let state = blank(fields.take("State")?);
    let units = units(&fields.take("Unit")?)?;
    let rx_type = blank(fields.take("RX Type")?);
    let comments = if fields.next_is("Comments") {
        let (first, more) = fields.take_with_lines("Comments")?;
        let lines: Vec<String> = std::iter::once(first).chain(more).filter(|line| !line.is_empty()).collect();
        blank(lines.join("\n"))
    } else {
        None
    };
    Ok((Site::Facility(Facility { name, units, rx_type, comments }), region, state))
}

/// A licensee's header lines, up to the notifier.
fn licensee(fields: &mut Fields) -> Res<(Site, u8, Option<String>)> {
    let rep_org = blank(fields.take("Rep Org")?);
    let name = blank(fields.take("Licensee")?);
    let region = region(&fields.take("Region")?)?;
    let city = blank(fields.take("City")?);
    let state = blank(fields.take("State")?);
    let county = blank(fields.take("County")?);
    let license = blank(fields.take("License #")?);
    let agreement = yes_no("Agreement", &fields.take("Agreement")?)?;
    let docket = blank(fields.take("Docket")?);
    let licensee = Licensee {
        rep_org,
        name,
        city,
        county,
        license,
        agreement,
        docket,
    };
    Ok((Site::Licensee(licensee), region, state))
}

fn region(value: &str) -> Res<u8> {
    value.parse::<u8>().ok().filter(|region| *region <= 4).with_context(|| format!("Region {value:?}"))
}

/// The units in `[1] [] [3]`: three brackets, each empty or holding a unit number.
fn units(value: &str) -> Res<Vec<u8>> {
    let parsed = || -> Option<Vec<u8>> {
        let groups: Vec<&str> = value.split_whitespace().collect();
        if groups.len() != 3 {
            return None;
        }
        let mut units = Vec::new();
        for group in groups {
            let inner = group.strip_prefix('[')?.strip_suffix(']')?;
            if !inner.is_empty() {
                units.push(inner.parse().ok()?);
            }
        }
        Some(units)
    };
    parsed().with_context(|| format!("Unit {value:?}"))
}

fn yes_no(label: &str, value: &str) -> Res<bool> {
    match value {
        "Y" => Ok(true),
        "N" => Ok(false),
        _ => bail!("{label} {value:?}"),
    }
}

/// The `<prefix> Date` and `<prefix> Time` lines.
fn stamp(fields: &mut Fields, prefix: &str) -> Res<Stamp> {
    let label = format!("{prefix} Date");
    let date = date(&label, &fields.take(&label)?)?;
    let label = format!("{prefix} Time");
    let value = fields.take(&label)?;
    // `05:12 [CST]`, with an empty zone on a few events.
    let parsed = || -> Option<(NaiveTime, Option<String>)> {
        let (clock, zone) = value.split_once('[')?;
        let zone = zone.strip_suffix(']')?.trim();
        let time = NaiveTime::parse_from_str(clock.trim(), "%H:%M").ok()?;
        zone.chars().all(|c| c.is_ascii_alphabetic()).then(|| (time, blank(zone.to_owned())))
    };
    let (time, zone) = parsed().with_context(|| format!("{label} {value:?}"))?;
    Ok(Stamp { date, time, zone })
}

fn date(label: &str, value: &str) -> Res<NaiveDate> {
    NaiveDate::parse_from_str(value, "%m/%d/%Y").with_context(|| format!("{label} {value:?}"))
}

/// The lines under "10 CFR Section": a code line ending in ` -` and its description, or (for
/// Agreement State events) a description alone.
fn sections(block: &str) -> Res<Vec<Section>> {
    let mut lines = block.lines().map(str::trim).filter(|line| !line.is_empty());
    let mut sections = Vec::new();
    while let Some(line) = lines.next() {
        let section = match line.strip_suffix(" -") {
            Some(code) => Section {
                code: Some(code.trim_end().to_owned()),
                description: lines.next().with_context(|| format!("10 CFR {code} has no description"))?.to_owned(),
            },
            None => Section { code: None, description: line.to_owned() },
        };
        sections.push(section);
    }
    Ok(sections)
}

fn unit_table(table: ElementRef<'_>, selectors: &Selectors) -> Res<Vec<UnitInfo>> {
    let caption = table.select(&selectors.caption).next().map(text_of);
    ensure!(caption.as_deref() == Some(UNIT_INFO), "unit table caption {caption:?}");
    let header: Vec<String> = table.select(&selectors.header).map(text_of).collect();
    ensure!(header.iter().eq(UNIT_HEADER.iter()), "unknown unit table header {header:?}");
    table
        .select(&selectors.row)
        .enumerate()
        .map(|(index, row)| {
            let mut cells: Vec<String> = row.select(&selectors.cell).map(text_of).collect();
            unit_row(&mut cells).with_context(|| format!("unit row {}", index + 1))
        })
        .collect()
}

fn unit_row(cells: &mut [String]) -> Res<UnitInfo> {
    let [unit, scram, critical, initial_power, initial_mode, current_power, current_mode] = cells else {
        bail!("{} cells under a {}-column header", cells.len(), UNIT_HEADER.len());
    };
    Ok(UnitInfo {
        unit: unit.parse().with_context(|| format!("unit {unit:?}"))?,
        scram: Scram::parse(scram)?,
        critical: yes_no("RX Crit", critical)?,
        initial_power: power(initial_power)?,
        initial_mode: blank(std::mem::take(initial_mode)),
        current_power: power(current_power)?,
        current_mode: blank(std::mem::take(current_mode)),
    })
}

fn power(value: &str) -> Res<u8> {
    let power: u8 = value.parse().with_context(|| format!("power {value:?}"))?;
    ensure!(power <= 100, "power {power} is over 100");
    Ok(power)
}

/// The event text: each `<br>` a line break, escaped ones included, the page source's own
/// newlines spaces (as a browser shows them), and each line trimmed.
fn event_text(body: ElementRef<'_>) -> Res<String> {
    let mut raw = String::new();
    for node in body.children() {
        match node.value() {
            Node::Text(text) => raw.extend(text.chars().map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })),
            Node::Element(element) if element.name() == "br" => raw.push('\n'),
            Node::Element(element) => bail!("unexpected <{}> in the event text", element.name()),
            _ => {}
        }
    }
    // Nine revision headers from December 2025 carry their line breaks escaped, as text.
    if raw.contains("<br>") {
        raw = raw.replace("<br>", "\n");
    }
    let lines: Vec<&str> = raw.lines().map(str::trim).collect();
    Ok(lines.join("\n").trim().to_owned())
}

/// The headline: the text's first line, past an `EN Revision Imported Date` header and with any
/// enclosing dashes trimmed. Text that opens on an update marker has none.
fn title(text: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let mut first = lines.next()?;
    if first.starts_with("EN Revision Imported Date") {
        first = lines.next()?;
    }
    if let Some(revised) = first.strip_prefix("EN Revision Text") {
        first = revised.trim_start_matches([' ', ':']);
    }
    if first.starts_with('*') {
        return None;
    }
    let title = first.trim_matches(|c: char| c == '-' || c.is_whitespace());
    (!title.is_empty()).then(|| title.to_owned())
}

/// The marker patterns, measured over the archive's 6,103 update and retraction markers
/// (PRD-0003.log.md#t-002). The `regex` crate has no lookaround, so each pattern consumes the
/// character on either side of its match and the fields come from named groups.
struct Markers {
    date: Regex,
    spelled: Regex,
    time: Regex,
    pair: Regex,
}

/// A month's name, abbreviated or not.
const MONTH: &str = "JAN(?:UARY)?|FEB(?:RUARY)?|MAR(?:CH)?|APR(?:IL)?|MAY|JUNE?|JULY?|AUG(?:UST)?|SEP(?:T|TEMBER)?|OCT(?:OBER)?|NOV(?:EMBER)?|DEC(?:EMBER)?";
const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];

impl Markers {
    fn new() -> Res<Self> {
        let compile = |pattern: &str| Regex::new(pattern).with_context(|| format!("compiling {pattern:?}"));
        Ok(Self {
            // Month, day, and a 2- or 4-digit year, not inside a longer run of digits and slashes.
            date: compile(r"(?:^|[^\d/])(?P<date>(?P<month>\d{1,2})/(?P<day>\d{1,2})/(?P<year>\d{4}|\d{2}))(?:[^\d/]|$)")?,
            // `MAY 28, 2021` or `14 APRIL 2021`, which markers have used since about 2018.
            spelled: compile(&format!(
                r"(?i)\b(?:(?P<day1>\d{{1,2}})\s+(?P<month1>{MONTH})\.?,?\s+(?P<year1>\d{{4}})|(?P<month2>{MONTH})\.?\s+(?P<day2>\d{{1,2}}),?\s+(?P<year2>\d{{4}}))\b"
            ))?,
            // A 24-hour time with or without its colon, then perhaps `HRS` and a zone.
            time: compile(r"(?i)(?:^|[^\d/:.])(?P<hour>[0-2]\d):?(?P<minute>[0-5]\d)(?:\s*(?:HRS?\.?\s*)?(?P<zone>AK[SD]T|A[SD]T|[ECMPHY][SD]?T)\b)?(?:\D|$)")?,
            // FROM or BY the caller, then TO, TAKEN BY or RECEIVED BY whoever took the call.
            pair: compile(r"(?i)\b(?:FROM|BY)\s+(?P<from>[^*@]+?),?\s+(?:TO|TAKEN BY|RECEIVED BY)\s+(?P<to>[^*@]+?)(?:\s+(?:AT|ON|VIA)\b|\s*[@*,]|\s*$)")?,
        })
    }

    /// The updates appended to `text`, in order. Each holds the lines after its marker up to the
    /// next one; the lines before the first marker are the report itself.
    fn updates(&self, text: &str) -> Vec<Update> {
        let mut blocks: Vec<(&str, Vec<&str>)> = Vec::new();
        for line in text.lines() {
            match (marker(line), blocks.last_mut()) {
                (Some(marker), _) => blocks.push((marker, Vec::new())),
                (None, Some((_, lines))) => lines.push(line),
                (None, None) => {}
            }
        }
        blocks
            .into_iter()
            .map(|(marker, lines)| Update {
                text: lines.join("\n").trim().to_owned(),
                ..self.update(marker)
            })
            .collect()
    }

    /// One marker's fields, with no text yet.
    fn update(&self, marker: &str) -> Update {
        let date = self.numeric_date(marker).or_else(|| self.spelled_date(marker));
        // The time and the names are read with the date taken out, so its digits read as neither.
        let rest = match &date {
            Some((_, range)) => Cow::Owned(format!("{} {}", &marker[..range.start], &marker[range.end..])),
            None => Cow::Borrowed(marker),
        };
        let time = self.time.captures_iter(&rest).find_map(|captures| {
            let time = NaiveTime::from_hms_opt(number(&captures, "hour")?, number(&captures, "minute")?, 0)?;
            Some((time, captures.name("zone").map(|zone| zone.as_str().to_ascii_uppercase())))
        });
        let pair = self.pair.captures(&rest);
        let name = |group: &str| {
            let name = pair.as_ref()?.name(group)?.as_str().trim().trim_end_matches(',').trim_end();
            (!name.is_empty()).then(|| name.to_owned())
        };
        let (time, zone) = time.map_or((None, None), |(time, zone)| (Some(time), zone));
        Update {
            kind: kind(marker),
            marker: marker.to_owned(),
            date: date.map(|(date, _)| date),
            time,
            zone,
            from: name("from"),
            to: name("to"),
            text: String::new(),
        }
    }
}

impl Markers {
    /// The first valid `M/D/YY` or `M/D/YYYY` date in `marker`, and where it sits.
    fn numeric_date(&self, marker: &str) -> Option<(NaiveDate, Range<usize>)> {
        self.date.captures_iter(marker).find_map(|captures| {
            let year = match number(&captures, "year")? {
                year @ 0..=49 => 2000 + year,
                year @ 50..=99 => 1900 + year,
                year => year,
            };
            let date = NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, number(&captures, "month")?, number(&captures, "day")?)?;
            Some((date, captures.name("date")?.range()))
        })
    }

    /// The first valid spelled-out date in `marker`, and where it sits.
    fn spelled_date(&self, marker: &str) -> Option<(NaiveDate, Range<usize>)> {
        self.spelled.captures_iter(marker).find_map(|captures| {
            let either = |first: &str, second: &str| captures.name(first).or_else(|| captures.name(second));
            let name = either("month1", "month2")?.as_str();
            let month = MONTHS.iter().position(|month| name.get(..3).is_some_and(|start| start.eq_ignore_ascii_case(month)))?;
            let day = either("day1", "day2")?.as_str().parse().ok()?;
            let year = either("year1", "year2")?.as_str().parse().ok()?;
            let date = NaiveDate::from_ymd_opt(year, u32::try_from(month + 1).ok()?, day)?;
            Some((date, captures.get(0)?.range()))
        })
    }
}

fn number(captures: &Captures<'_>, group: &str) -> Option<u32> {
    captures.name(group)?.as_str().parse().ok()
}

/// The marker on `line`, its stars trimmed: a line that opens with three or more stars, or one
/// that opens on an update or a retraction and closes with three or more (a handful lost their
/// opening stars). A line of stars alone is a rule in the text, not a marker.
fn marker(line: &str) -> Option<&str> {
    let stars = |chars: &mut dyn Iterator<Item = char>| chars.take_while(|c| *c == '*' || c.is_whitespace()).filter(|c| *c == '*').count();
    let body = line.trim_matches(|c: char| c == '*' || c.is_whitespace());
    if body.is_empty() {
        return None;
    }
    if stars(&mut line.chars()) >= 3 {
        return Some(body);
    }
    let opens = |word: &str| body.get(..word.len()).is_some_and(|start| start.eq_ignore_ascii_case(word));
    ((opens("UPDATE") || opens("RETRACT")) && stars(&mut line.chars().rev()) >= 3).then_some(body)
}

/// A marker's kind, from the first of its keywords that it holds, typos included.
fn kind(marker: &str) -> UpdateKind {
    let upper = marker.to_ascii_uppercase();
    if upper.contains("UNRETRACT") {
        UpdateKind::Unretraction
    } else if upper.contains("PARTIAL RETRACT") {
        UpdateKind::PartialRetraction
    } else if upper.contains("RETRACT") {
        UpdateKind::Retraction
    } else if ["UPDATE", "UDATE", "UDPATE"].iter().any(|word| upper.contains(word)) {
        UpdateKind::Update
    } else {
        UpdateKind::Note
    }
}

/// The date of an `EN Revision Imported Date: 9/29/2020` first line.
fn revised(text: &str) -> Res<Option<NaiveDate>> {
    let Some(rest) = text.lines().next().and_then(|line| line.strip_prefix("EN Revision Imported Date")) else {
        return Ok(None);
    };
    date("EN Revision Imported Date", rest.trim_start_matches([' ', ':']).trim()).map(Some)
}

/// One line of a field block: `<b>Label:</b> value`, or a line with no label, which continues the
/// labelled line above it (a facility's comments, the 10 CFR sections, the persons).
struct Field {
    label: Option<String>,
    value: String,
}

/// A field block's lines, taken in template order.
struct Fields {
    fields: Peekable<std::vec::IntoIter<Field>>,
}

impl Fields {
    /// Splits a block into lines at each `<br>`, starting a new one at each `<b>` label.
    fn of(block: ElementRef<'_>) -> Res<Self> {
        let mut fields = Vec::new();
        let mut current: Option<Field> = None;
        for node in block.children() {
            match node.value() {
                Node::Text(text) => match &mut current {
                    Some(field) => field.value.push_str(text),
                    None if text.trim().is_empty() => {}
                    None => current = Some(Field { label: None, value: text.to_string() }),
                },
                Node::Element(element) if element.name() == "b" => {
                    let label = ElementRef::wrap(node).map(text_of).unwrap_or_default();
                    let label = label.strip_suffix(':').with_context(|| format!("label {label:?} has no colon"))?.to_owned();
                    fields.extend(current.replace(Field { label: Some(label), value: String::new() }));
                }
                Node::Element(element) if element.name() == "br" => fields.extend(current.take()),
                Node::Element(element) => bail!("unexpected <{}> among the fields", element.name()),
                _ => {}
            }
        }
        fields.extend(current);
        for field in &mut fields {
            field.value = field.value.trim().to_owned();
        }
        Ok(Self { fields: fields.into_iter().peekable() })
    }

    /// The next line's value, which must carry `label`.
    fn take(&mut self, label: &str) -> Res<String> {
        match self.fields.next() {
            Some(Field { label: Some(found), value }) if found == label => Ok(value),
            Some(Field { label: Some(found), .. }) => bail!("expected {label}, found {found}"),
            Some(Field { label: None, value }) => bail!("expected {label}, found {value:?}"),
            None => bail!("expected {label}, found nothing"),
        }
    }

    /// The next line's value, which must carry `label`, and the unlabelled lines after it.
    fn take_with_lines(&mut self, label: &str) -> Res<(String, Vec<String>)> {
        let value = self.take(label)?;
        let mut lines = Vec::new();
        while let Some(field) = self.fields.next_if(|field| field.label.is_none()) {
            lines.push(field.value);
        }
        Ok((value, lines))
    }

    fn next_is(&mut self, label: &str) -> bool {
        self.fields.peek().is_some_and(|field| field.label.as_deref() == Some(label))
    }

    fn end(mut self) -> Void {
        match self.fields.next() {
            None => Ok(()),
            Some(Field { label: Some(label), .. }) => bail!("unexpected field {label}"),
            Some(Field { label: None, value }) => bail!("unexpected line {value:?}"),
        }
    }
}

/// The element children of `parent`, which may hold nothing else but whitespace and comments.
fn elements(parent: ElementRef<'_>) -> Res<Vec<ElementRef<'_>>> {
    let mut elements = Vec::new();
    for node in parent.children() {
        match node.value() {
            Node::Element(_) => elements.extend(ElementRef::wrap(node)),
            Node::Text(text) if text.trim().is_empty() => {}
            Node::Text(text) => bail!("stray text {:?} in <{}>", text.trim(), parent.value().name()),
            _ => {}
        }
    }
    Ok(elements)
}

fn is(element: ElementRef<'_>, name: &str) -> bool {
    element.value().name() == name
}

fn is_class(element: ElementRef<'_>, name: &str, class: &str) -> bool {
    is(element, name) && element.value().classes().any(|found| found == class)
}

/// An element's text with entities decoded and the ends trimmed.
fn text_of(element: ElementRef<'_>) -> String {
    let text: String = element.text().collect();
    text.trim().to_owned()
}

fn blank(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::fmt::Write as _;

    use pretty_assertions::assert_eq;

    use super::*;

    const FACILITY: &str = "<b>Facility:</b> Point Beach<br />
            <b>Region:</b> 3 &nbsp; &nbsp; <b>State:</b> WI<br>
            <b>Unit:</b> [1] [] []<br />
            <b>RX Type:</b> [1] W-2-LP,[2] W-2-LP<br />
                                <b>NRC Notified By:</b> Sam Hall<br>
          <b>HQ OPS Officer:</b> Adam Koziol";

    const LICENSEE: &str = "<b>Rep Org:</b> Ohio Department of Health<br>
            <b>Licensee:</b> Cardiovascular Med. of Cleveland<br>
            <b>Region:</b> 3<br>
            <b>City:</b> Fairview Park &nbsp; <b>State:</b> OH<br>
            <b>County:</b>  <br>
            <b>License #:</b> 02220180002<br>
            <b>Agreement:</b> Y<br>
            <b>Docket:</b> <br>
                    <b>NRC Notified By:</b> Joshua A. Wagner<br>
          <b>HQ OPS Officer:</b> Josue Ramirez";

    const DATES: &str = "<b>Notification Date:</b> 11/10/2026<br>
          <b>Notification Time:</b> 09:45 [ET]<br>
          <b>Event Date:</b> 11/09/2026<br>
          <b>Event Time:</b> 05:12 [CST]<br>
          <b>Last Update Date:</b> 11/12/2026";

    const SCRAM: &str = "<b>Emergency Class:</b> Non Emergency<br>
          10 CFR Section:<br>
                                                              50.72(b)(2)(iv)(B) -
                            RPS Actuation - Critical
                                                    ";

    const PERSONS: &str = "<b>Person (Organization):</b><br>
                      Ruiz, Robert (R3 DRPBC)<br ?>
                                R3 Reactor Event (Email)<br ?>
                                                ";

    const TEXT: &str = "AUTOMATIC REACTOR TRIP<br />
<br />
&quot; At 0512 CST, with Unit 1 at 100 percent power &amp; rising, the reactor tripped.<br />
 <br />
&quot;The Resident Inspector has been notified.&quot;<br />
";

    /// One event's markup, field block by field block.
    #[derive(Clone)]
    struct Parts {
        number: u32,
        category: &'static str,
        retracted: bool,
        site: &'static str,
        dates: &'static str,
        class: &'static str,
        persons: &'static str,
        table: Option<Vec<[&'static str; 7]>>,
        text: &'static str,
    }

    impl Parts {
        fn reactor() -> Self {
            Self {
                number: 58526,
                category: "Power Reactor",
                retracted: false,
                site: FACILITY,
                dates: DATES,
                class: SCRAM,
                persons: PERSONS,
                table: Some(vec![["1", "A/R", "Y", "100", "Power Operation", "0", "Hot Standby"]]),
                text: TEXT,
            }
        }

        fn agreement() -> Self {
            Self {
                number: 58518,
                category: "Agreement State",
                site: LICENSEE,
                class: "<b>Emergency Class:</b> Non Emergency<br>\n 10 CFR Section:<br>\n    Agreement State\n   ",
                table: None,
                text: "AGREEMENT STATE REPORT - LOST SOURCES<br />\n<br />\nTwo Cs-137 sources were lost.",
                ..Self::reactor()
            }
        }

        fn category(self, category: &'static str) -> Self {
            Self { category, ..self }
        }

        fn site(self, site: &'static str) -> Self {
            Self { site, ..self }
        }

        fn dates(self, dates: &'static str) -> Self {
            Self { dates, ..self }
        }

        fn class(self, class: &'static str) -> Self {
            Self { class, ..self }
        }

        fn persons(self, persons: &'static str) -> Self {
            Self { persons, ..self }
        }

        fn table(self, table: Option<Vec<[&'static str; 7]>>) -> Self {
            Self { table, ..self }
        }

        fn text(self, text: &'static str) -> Self {
            Self { text, ..self }
        }

        fn html(&self) -> String {
            let mut html = format!(r#"<div class="grid border" id="en{}">"#, self.number);
            if self.retracted {
                for _ in 0..2 {
                    html.push_str(r#"<div class="retracted">!!!!! THIS EVENT HAS BEEN RETRACTED !!!!!</div>"#);
                }
            }
            write!(
                html,
                r#"<div class="th">{}</div>
        <div class="th">Event Number: {}</div>
        <div>
                      {}
        </div>
        <div>
          {}
        </div>
        <div>
          {}</div>
        <div>
          {}</div>
      </div>
"#,
                self.category, self.number, self.site, self.dates, self.class, self.persons
            )
            .unwrap();
            if let Some(rows) = &self.table {
                html.push_str(
                    "<table><caption>Power Reactor Unit Info</caption><thead><tr><th>Unit</th><th>SCRAM Code</th><th>RX Crit</th>\
                     <th>Initial PWR</th><th>Initial RX Mode</th><th>Current PWR</th><th>Current RX Mode</th></tr></thead><tbody>",
                );
                for row in rows {
                    html.push_str("<tr>");
                    for cell in row {
                        write!(html, "<td>{cell}</td>").unwrap();
                    }
                    html.push_str("</tr>");
                }
                html.push_str("</tbody></table>");
            }
            write!(html, "\n<b>Event Text</b>\n<div class=\"border\">\n        {}\n      </div>\n      <br /><br />\n", self.text).unwrap();
            html
        }
    }

    const HEAD: &str = r#"<!DOCTYPE html><html><head><title>EN</title></head><body><main><h1>Event Notification Report</h1>
<div class="nrc-event-report-day general-content">
  <div class="event-summary text-center">
    <p>U.S. Nuclear Regulatory Commission<br>Operations Center</p>
    <p>EVENT REPORTS FOR<br>11/11/2026 - 11/12/2026 </p>
  </div>
"#;

    /// A page in the live template, its summary listing `numbers`.
    fn page_listing(numbers: &[u32], events: &[Parts]) -> String {
        let mut html = HEAD.to_owned();
        html.push_str(r#"<p class="event-summary number text-center"><b>EVENT NUMBERS</b><br>"#);
        for number in numbers {
            write!(html, r##"<a href="#en{number}">{number}</a>"##).unwrap();
        }
        html.push_str("</p>");
        for event in events {
            html.push_str(&event.html());
        }
        html + "</div></main><footer>footer</footer></body></html>"
    }

    /// A page in the live template.
    fn page(events: &[Parts]) -> String {
        let numbers: Vec<u32> = events.iter().map(|event| event.number).collect();
        page_listing(&numbers, events)
    }

    fn one(parts: Parts) -> Event {
        let mut events = parse(page(&[parts]).as_bytes()).unwrap();
        assert_eq!(events.len(), 1);
        events.remove(0)
    }

    fn error(page: &str) -> String {
        format!("{:#}", parse(page.as_bytes()).unwrap_err())
    }

    fn day(date: &str) -> NaiveDate {
        date.parse().unwrap()
    }

    fn stamp(date: &str, time: &str, zone: Option<&str>) -> Stamp {
        Stamp {
            date: day(date),
            time: NaiveTime::parse_from_str(time, "%H:%M").unwrap(),
            zone: zone.map(str::to_owned),
        }
    }

    #[test]
    fn a_reactor_event_parses_every_field() {
        let event = one(Parts::reactor());

        assert_eq!(
            event,
            Event {
                number: 58526,
                category: Category::PowerReactor,
                retracted: false,
                site: Site::Facility(Facility {
                    name: "Point Beach".to_owned(),
                    units: vec![1],
                    rx_type: Some("[1] W-2-LP,[2] W-2-LP".to_owned()),
                    comments: None,
                }),
                region: 3,
                state: Some("WI".to_owned()),
                notified_by: Some("Sam Hall".to_owned()),
                ops_officer: Some("Adam Koziol".to_owned()),
                notified: stamp("2026-11-10", "09:45", Some("ET")),
                occurred: stamp("2026-11-09", "05:12", Some("CST")),
                updated: day("2026-11-12"),
                class: Some(EmergencyClass::NonEmergency),
                sections: vec![Section {
                    code: Some("50.72(b)(2)(iv)(B)".to_owned()),
                    description: "RPS Actuation - Critical".to_owned(),
                }],
                persons: vec!["Ruiz, Robert (R3 DRPBC)".to_owned(), "R3 Reactor Event (Email)".to_owned()],
                units: vec![UnitInfo {
                    unit: 1,
                    scram: Some(Scram::AutoReactor),
                    critical: true,
                    initial_power: 100,
                    initial_mode: Some("Power Operation".to_owned()),
                    current_power: 0,
                    current_mode: Some("Hot Standby".to_owned()),
                }],
                title: Some("AUTOMATIC REACTOR TRIP".to_owned()),
                text: "AUTOMATIC REACTOR TRIP\n\n\" At 0512 CST, with Unit 1 at 100 percent power & rising, the reactor tripped.\n\n\
                       \"The Resident Inspector has been notified.\""
                    .to_owned(),
                updates: vec![],
                revised: None,
            }
        );
    }

    #[test]
    fn a_licensee_event_parses_every_field() {
        let event = one(Parts::agreement());

        assert_eq!(
            event.site,
            Site::Licensee(Licensee {
                rep_org: Some("Ohio Department of Health".to_owned()),
                name: Some("Cardiovascular Med. of Cleveland".to_owned()),
                city: Some("Fairview Park".to_owned()),
                county: None,
                license: Some("02220180002".to_owned()),
                agreement: true,
                docket: None,
            })
        );
        assert_eq!((event.category, event.region, event.state.as_deref()), (Category::AgreementState, 3, Some("OH")));
        assert_eq!(
            event.sections,
            [Section {
                code: None,
                description: "Agreement State".to_owned()
            }]
        );
        assert_eq!(event.units, []);
        assert_eq!(event.title.as_deref(), Some("AGREEMENT STATE REPORT - LOST SOURCES"));
    }

    #[test]
    fn every_category_label_parses() {
        for category in Category::ALL {
            let parts = match category {
                Category::PowerReactor => Parts::reactor(),
                Category::FuelCycleFacility => Parts { table: None, ..Parts::reactor() },
                _ => Parts::agreement(),
            };

            let event = one(Parts { category: category.label(), ..parts });

            assert_eq!(event.category, category);
        }
    }

    #[test]
    fn comments_keep_one_line_per_line() {
        let site = "<b>Facility:</b> PORTSMOUTH GASEOUS DIFFUSION PLANT<br />
            <b>Region:</b> 3 &nbsp; &nbsp; <b>State:</b> OH<br>
            <b>Unit:</b> [] [] []<br />
            <b>RX Type:</b> URANIUM ENRICHMENT FACILITY<br />
            <b>Comments:</b>
            2 DEMOCRACY CENTER<br />
            6903 ROCKLEDGE DRIVE<br />
            BETHESDA, MD 20817 (301)564-3200<br />
                                <b>NRC Notified By:</b> RICHARD LARSON<br>
          <b>HQ OPS Officer:</b> STEVE SANDIN";

        let event = one(Parts {
            category: "Fuel Cycle Facility",
            site,
            table: None,
            ..Parts::reactor()
        });

        assert_eq!(
            event.site,
            Site::Facility(Facility {
                name: "PORTSMOUTH GASEOUS DIFFUSION PLANT".to_owned(),
                units: vec![],
                rx_type: Some("URANIUM ENRICHMENT FACILITY".to_owned()),
                comments: Some("2 DEMOCRACY CENTER\n6903 ROCKLEDGE DRIVE\nBETHESDA, MD 20817 (301)564-3200".to_owned()),
            })
        );
    }

    #[test]
    fn units_list_the_bracketed_numbers() {
        let site = FACILITY.replace("[1] [] []", "[] [3] [4]");

        let event = one(Parts { site: site.leak(), ..Parts::reactor() });

        let Site::Facility(facility) = event.site else { panic!("not a facility") };
        assert_eq!(facility.units, [3, 4]);
    }

    #[test]
    fn sections_pair_each_code_with_its_description() {
        let cases: [(&str, Vec<Section>); 3] = [
            (
                "<b>Emergency Class:</b> NON EMERGENCY<br>\n 10 CFR Section:<br>\n  50.72(b)(2)(iv)(B) -\n  RPS ACTUATION - CRITICAL\n   50.72(a) (1) (i) -\n   EMERGENCY DECLARED\n  ",
                vec![
                    Section {
                        code: Some("50.72(b)(2)(iv)(B)".to_owned()),
                        description: "RPS ACTUATION - CRITICAL".to_owned(),
                    },
                    Section {
                        code: Some("50.72(a) (1) (i)".to_owned()),
                        description: "EMERGENCY DECLARED".to_owned(),
                    },
                ],
            ),
            (
                "<b>Emergency Class:</b> NON EMERGENCY<br>\n 10 CFR Section:<br>\n  20.2201(a)(1)(i) -\n  LOST/STOLEN LNM&gt;1000X\n",
                vec![Section {
                    code: Some("20.2201(a)(1)(i)".to_owned()),
                    description: "LOST/STOLEN LNM>1000X".to_owned(),
                }],
            ),
            ("<b>Emergency Class:</b> NON EMERGENCY<br>\n 10 CFR Section:<br>\n     \n", vec![]),
        ];
        for (class, sections) in cases {
            assert_eq!(one(Parts { class, ..Parts::reactor() }).sections, sections, "{class:?}");
        }
    }

    #[test]
    fn emergency_classes_parse_in_either_case_and_may_be_blank() {
        let cases = [
            ("NON EMERGENCY", Some(EmergencyClass::NonEmergency)),
            ("Unusual Event", Some(EmergencyClass::UnusualEvent)),
            ("ALERT", Some(EmergencyClass::Alert)),
            ("SITE AREA EMERGENCY", Some(EmergencyClass::SiteAreaEmergency)),
            ("General Emergency", Some(EmergencyClass::GeneralEmergency)),
            ("", None),
        ];
        for (written, class) in cases {
            let html = SCRAM.replace("Non Emergency", written);

            assert_eq!(one(Parts { class: html.leak(), ..Parts::reactor() }).class, class, "{written:?}");
        }
    }

    #[test]
    fn scram_codes_parse_as_written() {
        let cases = [
            ("N", Some(Scram::No)),
            ("A/R", Some(Scram::AutoReactor)),
            ("M/R", Some(Scram::ManualReactor)),
            ("A", Some(Scram::Auto)),
            ("M", Some(Scram::Manual)),
            ("", None),
        ];
        for (code, scram) in cases {
            let table = Some(vec![["2", code, "N", "0", "", "0", "Cold Shutdown"]]);

            let units = one(Parts { table, ..Parts::reactor() }).units;

            assert_eq!(units[0].scram, scram, "{code:?}");
            assert_eq!((units[0].unit, units[0].critical, units[0].initial_mode.as_deref()), (2, false, None));
        }
    }

    #[test]
    fn a_blank_time_zone_reads_as_none() {
        let dates = DATES.replace("05:12 [CST]", "00:00 []");

        assert_eq!(one(Parts { dates: dates.leak(), ..Parts::reactor() }).occurred, stamp("2026-11-09", "00:00", None));
    }

    #[test]
    fn a_nul_placeholder_reads_as_blank() {
        let site = LICENSEE.replace("<b>County:</b>  <br>", "<b>County:</b> \0<br>");

        let event = one(Parts { site: site.leak(), ..Parts::agreement() });

        let Site::Licensee(licensee) = event.site else { panic!("not a licensee") };
        assert_eq!(licensee.county, None);
    }

    #[test]
    fn a_retraction_banner_marks_the_event() {
        assert!(one(Parts { retracted: true, ..Parts::reactor() }).retracted);
    }

    #[test]
    fn several_events_parse_in_page_order() {
        let events = parse(page(&[Parts::agreement(), Parts::reactor()]).as_bytes()).unwrap();

        assert_eq!(events.iter().map(|event| event.number).collect::<Vec<_>>(), [58518, 58526]);
    }

    #[test]
    fn a_page_with_no_events_is_empty() {
        let html = format!(r#"{HEAD}<div class="text-align-center"><strong>No events found</strong></div></div></main></body></html>"#);

        assert_eq!(parse(html.as_bytes()).unwrap(), []);
    }

    #[test]
    fn the_title_is_the_first_line() {
        let cases = [
            ("AUTOMATIC REACTOR TRIP\n\nAt 0512 ...", Some("AUTOMATIC REACTOR TRIP")),
            ("- FITNESS FOR DUTY REPORT -\n\nA licensed operator ...", Some("FITNESS FOR DUTY REPORT")),
            ("MANUAL SCRAM DUE TO RISING WATER LEVEL\nThe following ...", Some("MANUAL SCRAM DUE TO RISING WATER LEVEL")),
            (
                "EN Revision Imported Date: 12/8/2020\n\nEN Revision Text: AGREEMENT STATE REPORT - DAMAGED GAUGE\n\nThe following ...",
                Some("AGREEMENT STATE REPORT - DAMAGED GAUGE"),
            ),
            ("EN Revision Imported Date : 10/2/2020\n\nEN Revision Text : OFFSITE NOTIFICATION\n\n...", Some("OFFSITE NOTIFICATION")),
            ("* * * * * * * * * *\nTHIS IS NOT A NEW REPORT.", None),
            ("", None),
        ];
        for (text, expected) in cases {
            assert_eq!(title(text).as_deref(), expected, "{text:?}");
        }
    }

    /// Asserts that `page` fails to parse, naming `expected`.
    fn fails(page: &str, expected: &str) {
        let error = error(page);
        assert!(error.contains(expected), "{expected:?} not in {error:?}");
    }

    #[test]
    fn the_page_layout_is_enforced() {
        let reactor = Parts::reactor;
        let cases = [
            (page_listing(&[1], &[reactor()]), "the summary lists [1] but the page holds [58526]"),
            (page(&[reactor()]).replace(r#"id="en58526""#, r#"id="en1""#), "Event Number: 58526 under id \"en1\""),
            (page(&[reactor()]).replace("<b>Event Text</b>", "<b>Text</b>"), "expected <b>Event Text</b>"),
            (format!("{HEAD}</div></main>"), "neither events nor \"No events found\""),
            ("<html><body><table></table></body></html>".to_owned(), "expected exactly one div.nrc-event-report-day"),
        ];
        for (html, expected) in cases {
            fails(&html, expected);
        }
    }

    #[test]
    fn the_event_layout_is_enforced() {
        let (reactor, agreement) = (Parts::reactor, Parts::agreement);
        let cases = [
            (reactor().category("Power Plant"), "unknown category \"Power Plant\""),
            (reactor().table(None), "no Power Reactor Unit Info table"),
            (agreement().table(Some(vec![])), "a unit table on a Agreement State event"),
            (reactor().site(LICENSEE), "expected Facility, found Rep Org"),
            (agreement().site(FACILITY), "expected Rep Org, found Facility"),
            (reactor().site("<b>Facility:</b> X<br><b>Owner:</b> Y"), "expected Region, found Owner"),
            (reactor().class("<b>Emergency Class:</b> ALERT<br>\n 50.72 -\n"), "expected \"10 CFR Section:\""),
            (reactor().persons("<b>Person (Organization):</b> Ruiz<br>"), "Person (Organization) \"Ruiz\""),
            (reactor().text("a <i>slanted</i> word"), "unexpected <i> in the event text"),
            (reactor().site("<b>Facility:</b> X <i>Y</i>"), "unexpected <i> among the fields"),
        ];
        for (parts, expected) in cases {
            fails(&page(&[parts]), expected);
        }
    }

    #[test]
    fn values_that_do_not_parse_fail() {
        let reactor = Parts::reactor;
        let row = |cells: [&'static str; 7]| reactor().table(Some(vec![cells]));
        let cases = [
            (reactor().dates("<b>Notification Date:</b> 2026-11-10<br>"), "Notification Date \"2026-11-10\""),
            (reactor().dates(DATES.replace("05:12", "5 AM").leak()), "Event Time \"5 AM [CST]\""),
            (reactor().class("<b>Emergency Class:</b> Code Red<br>\n 10 CFR Section:<br>\n"), "emergency class \"Code Red\""),
            (reactor().class("<b>Emergency Class:</b> ALERT<br>\n 10 CFR Section:<br>\n 50.72(a) -\n"), "50.72(a) has no description"),
            (reactor().site(FACILITY.replace("[1] [] []", "[1] [2]").leak()), "Unit \"[1] [2]\""),
            (reactor().site(FACILITY.replace("Region:</b> 3", "Region:</b> 5").leak()), "Region \"5\""),
            (Parts::agreement().site(LICENSEE.replace("Agreement:</b> Y", "Agreement:</b> ?").leak()), "Agreement \"?\""),
            (row(["1", "X", "Y", "100", "", "100", ""]), "scram code \"X\""),
            (row(["1", "N", "Y", "101", "", "100", ""]), "power 101 is over 100"),
            (row(["1", "N", "?", "100", "", "100", ""]), "RX Crit \"?\""),
        ];
        for (parts, expected) in cases {
            fails(&page(&[parts]), expected);
        }
    }

    #[test]
    fn errors_name_the_event() {
        let bad = Parts {
            dates: DATES.replace("11/09/2026", "11/31/2026").leak(),
            ..Parts::reactor()
        };

        assert_eq!(error(&page(&[Parts::agreement(), bad])), "event 58526: Event Date \"11/31/2026\": input is out of range");
    }

    #[test]
    fn a_page_that_is_not_utf8_fails() {
        assert!(parse(b"\xff\xfe").unwrap_err().to_string().contains("not UTF-8"));
    }

    fn updates(text: &str) -> Vec<Update> {
        Parser::new().unwrap().markers.updates(text)
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

    /// A marker line in each variant the archive holds, and its fields as `summary` writes them.
    const MARKERS: [(&str, &str); 35] = [
        (
            "* * * UPDATE AT 1949 EST ON 01/01/99 BY ALAN BRIESE TO FANGIE JONES * * *",
            "Update 1999-01-01 19:49 EST | ALAN BRIESE | FANGIE JONES",
        ),
        (
            "* * * UPDATE 1835 EST ON 2/3/99 FROM HOWARD SMITH TO S.SANDIN * * *",
            "Update 1999-02-03 18:35 EST | HOWARD SMITH | S.SANDIN",
        ),
        (
            "* * * UPDATE at 1047 on 01/27/99 by Fred Smith to Dick Jolliffe * * *",
            "Update 1999-01-27 10:47 - | Fred Smith | Dick Jolliffe",
        ),
        (
            "***RETRACTION ON 03/05/99 AT 1532ET BY ANDREW WISNIEWSKI TAKEN BY MACKINNON***",
            "Retraction 1999-03-05 15:32 ET | ANDREW WISNIEWSKI | MACKINNON",
        ),
        (
            "****Update on 01/21/99 at 1602 ET by Bannister taken by MacKinnon****",
            "Update 1999-01-21 16:02 ET | Bannister | MacKinnon",
        ),
        (
            "* * * RETRACTED ON 2/2/99 AT 1122 EST BY MIKE MEYER TO FANGIE JONES * * *",
            "Retraction 1999-02-02 11:22 EST | MIKE MEYER | FANGIE JONES",
        ),
        (
            "* * * UPDATED AT 1633 EST ON 01/03/2001 BY ROBERT ALLEN TO FANGIE JONES * * *",
            "Update 2001-01-03 16:33 EST | ROBERT ALLEN | FANGIE JONES",
        ),
        ("* * * UPDATEON 09/20/03 @ 1000 BY KOPCHICK TO GOULD * * *", "Update 2003-09-20 10:00 - | KOPCHICK | GOULD"),
        (
            "* * * UDATE ON 01/29/03 AT 1414 EST FROM DON ROLAND TO GERRY WAIG * * *",
            "Update 2003-01-29 14:14 EST | DON ROLAND | GERRY WAIG",
        ),
        (
            "* * * UDPATE ON 02/27/07 AT 0950 EST FROM ARTHUR TUCKER VIA E-MAIL TO MACKINNON * * *",
            "Update 2007-02-27 09:50 EST | ARTHUR TUCKER VIA E-MAIL | MACKINNON",
        ),
        (
            "* * * UPDATE ON 3/1/99 @ 1817 BY POWERS TO GOULD ***  EVENT RETRACTION",
            "Retraction 1999-03-01 18:17 - | POWERS | GOULD",
        ),
        (
            "* * * EVENT RETRACTED AT 1743 EST 3/7/02 BY GRANT FERNSLER TO GERRY WAIG * * *",
            "Retraction 2002-03-07 17:43 EST | GRANT FERNSLER | GERRY WAIG",
        ),
        ("* * * THIS EVENT IS BEING RETRACTED ON 12/20/05 AT 1557 * * *", "Retraction 2005-12-20 15:57 - | - | -"),
        (
            "* * * PARTIAL RETRACTION ON 5/15/2020 AT 1442 EDT FROM BUD HINCKLEY TO THOMAS HERRITY * * *",
            "PartialRetraction 2020-05-15 14:42 EDT | BUD HINCKLEY | THOMAS HERRITY",
        ),
        (
            "* * * UNRETRACTION ON 06/22/05 AT 0945 EDT BY MARTY JOHNSON TO HOWIE CROUCH * * *",
            "Unretraction 2005-06-22 09:45 EDT | MARTY JOHNSON | HOWIE CROUCH",
        ),
        (
            "* * * RETRACTION AT 12:42 ON 8/3/99 FROM DORAZIO TO POERTNER * * *",
            "Retraction 1999-08-03 12:42 - | DORAZIO | POERTNER",
        ),
        ("* * * UPDATE 0818 1/18/2001FROM OGDEN TAKEN BY STRANSKY * * *", "Update 2001-01-18 08:18 - | OGDEN | STRANSKY"),
        ("* * * UPDATE AT 1545 BY PROPPER, TAKEN BY WEAVER * * *", "Update - 15:45 - | PROPPER | WEAVER"),
        ("* * * UPDATE AT 1701 ON 1/20/99, BY SWAFFORD TAKEN BY WEAVER * * *", "Update 1999-01-20 17:01 - | SWAFFORD | WEAVER"),
        ("* * * UPDATE AT 2257 ON 09//22/99 BY JOE HALCOMB TAKEN BY JOLLIFFE * * *", "Update - 22:57 - | JOE HALCOMB | JOLLIFFE"),
        (
            "* * * UPDATE FROM GREG MILLER TO HOWIE CROUCH @ 1534 ON 8/25/09 * * *",
            "Update 2009-08-25 15:34 - | GREG MILLER | HOWIE CROUCH",
        ),
        (
            "* * * UPDATE RECEIVED VIA EMAIL FROM PAUL STEPANTSCHENKO TO DONG PARK AT 1642 EST ON 12/01/09 * * *",
            "Update 2009-12-01 16:42 EST | PAUL STEPANTSCHENKO | DONG PARK",
        ),
        (
            "* * * 1058 EDT ON 7/11/2019, UPDATE FROM RANDY IANTORNO (CURTIS-WRIGHT) TO MICHAEL BLOODGOOD * * *",
            "Update 2019-07-11 10:58 EDT | RANDY IANTORNO (CURTIS-WRIGHT) | MICHAEL BLOODGOOD",
        ),
        (
            "******************** UPDATE AT 1653 ON 04/27/99 FROM JEFF HAVENER TO TROCINE ********************",
            "Update 1999-04-27 16:53 - | JEFF HAVENER | TROCINE",
        ),
        (
            "* * * Updated on 5/10/00 to reflect correct reactor mode of cold shutdown vice hot shutdown * * *",
            "Update 2000-05-10 - - | - | -",
        ),
        ("UPDATE ON 3/31/2000 BY LAMB, RECEIVED BY WEAVER * * *", "Update 2000-03-31 - - | LAMB | WEAVER"),
        (
            "RETRACTION ON 11/18/2008 AT 1214 FROM RANDY WROBLEWSKI TO MARK ABRAMOVITZ * * *",
            "Retraction 2008-11-18 12:14 - | RANDY WROBLEWSKI | MARK ABRAMOVITZ",
        ),
        (
            "* * * UPDATE ON MAY 28, 2021 AT 1603 EDT FROM ANTHONY CARPENITO TO BRIAN P. SMITH * * *",
            "Update 2021-05-28 16:03 EDT | ANTHONY CARPENITO | BRIAN P. SMITH",
        ),
        (
            "* * * RETRACTION ON 14 APRIL 2021 AT 1522 EDT FROM BROOKE OLSON TO JOANNA BRIDGE * * *",
            "Retraction 2021-04-14 15:22 EDT | BROOKE OLSON | JOANNA BRIDGE",
        ),
        (
            "* * * UPDATE ON SEPTEMBER 22, 2026, AT 1102 EDT FROM NATHANIEL KISHBAUGH TO ERNEST WEST * * *",
            "Update 2026-09-22 11:02 EDT | NATHANIEL KISHBAUGH | ERNEST WEST",
        ),
        (
            "* * * UPDATE FROM STEVE MACK TO DONALD NORWOOD VIA E-MAIL ON APRIL 27, 2009 AT 1552 HOURS * * *",
            "Update 2009-04-27 15:52 - | STEVE MACK | DONALD NORWOOD",
        ),
        (
            "* * * RETRACTION FROM KIM HOFFMAN TO JOHN SHOEMAKER AT 1303 EST ON 1/17/18/17 * * *",
            "Retraction - 13:03 EST | KIM HOFFMAN | JOHN SHOEMAKER",
        ),
        ("* * * COMMISSIONERS' ASSISTANTS' BRIEFING HELD AT 1430 EDT 8/31/1999 * * *", "Note 1999-08-31 14:30 EDT | - | -"),
        (
            "********** HOO NOTE:  Based on plant status calls that occurred on 05/15/99 and 05/16/99, the cause",
            "Note 1999-05-15 - - | - | -",
        ),
        (
            "* * * CLOSEOUT  RECEIVED AT 1642 EST ON 3/4/20119 FROM SHELDON THOMAS TO MARK ABRAMOVITZ * * *",
            "Note - 16:42 EST | SHELDON THOMAS | MARK ABRAMOVITZ",
        ),
    ];

    #[test]
    fn markers_in_every_recorded_variant_parse() {
        for (line, expected) in MARKERS {
            let updates = updates(&format!("THE REPORT\n\n{line}\n\nMore."));

            assert_eq!(updates.iter().map(summary).collect::<Vec<_>>(), [expected], "{line:?}");
        }
    }

    #[test]
    fn lines_that_are_not_markers_stay_text() {
        let text = "THE REPORT\n\
                    * * * * * * * * * * * * * * * * * * * *\n\
                    THIS IS NOT A NEW REPORT.\n\
                    \"***This event did not result in any offsite release.***\"\n\
                    Cs-137    Berthold    LB7440  ***    50 mCi\n\
                    \"Beaver Valley Unit 2 has terminated the Unusual Event at 0955 EDT * * *\n\
                    *Starred note\n\
                    THE UPDATE WAS LATE * * *";

        assert_eq!(updates(text), []);
    }

    #[test]
    fn each_update_holds_its_lines_up_to_the_next_marker() {
        let text = "LOSS OF POWER\n\nThe report.\n\n\
                    * * * UPDATE AT 1949 EST ON 01/01/99 BY ALAN BRIESE TO FANGIE JONES * * *\n\n\
                    First update.\n\n* * * * * * * *\n\nSecond paragraph.\n\n\
                    * * * RETRACTION ON 01/02/99 AT 0800 EST FROM BRIESE TO JONES * * *\n\nRetracted.";

        let updates = updates(text);

        let texts: Vec<(&str, &str)> = updates.iter().map(|update| (update.marker.as_str(), update.text.as_str())).collect();
        assert_eq!(
            texts,
            [
                ("UPDATE AT 1949 EST ON 01/01/99 BY ALAN BRIESE TO FANGIE JONES", "First update.\n\n* * * * * * * *\n\nSecond paragraph."),
                ("RETRACTION ON 01/02/99 AT 0800 EST FROM BRIESE TO JONES", "Retracted."),
            ]
        );
    }

    #[test]
    fn a_revision_import_is_a_note_not_an_update() {
        let cases = [
            ("EN Revision Imported Date: 12/8/2020\n\nEN Revision Text: DAMAGED GAUGE\n\nThe following ...", Some("2020-12-08")),
            ("EN Revision Imported Date : 10/2/2020\n\nEN Revision Text : OFFSITE NOTIFICATION", Some("2020-10-02")),
            ("AUTOMATIC REACTOR TRIP\n\nEN Revision Imported Date: 12/8/2020", None),
        ];
        for (text, expected) in cases {
            assert_eq!(revised(text).unwrap(), expected.map(day), "{text:?}");
            assert_eq!(updates(text), [], "{text:?}");
        }
    }

    #[test]
    fn an_escaped_line_break_in_the_text_is_a_line_break() {
        let text = "EN Revision Imported Date: 12/11/2025&lt;br&gt;&lt;br&gt;EN Revision Text: LEAKING SOURCE<br />\n<br />\nThe body.";

        let event = one(Parts::agreement().text(text));

        assert_eq!(event.text, "EN Revision Imported Date: 12/11/2025\n\nEN Revision Text: LEAKING SOURCE\n\nThe body.");
        assert_eq!((event.title.as_deref(), event.revised), (Some("LEAKING SOURCE"), Some(day("2025-12-11"))));
    }

    #[test]
    fn a_revision_date_that_does_not_parse_fails() {
        let error = format!("{:#}", revised("EN Revision Imported Date: 13/45/2020\n\nEN Revision Text: X").unwrap_err());

        assert!(error.contains("EN Revision Imported Date \"13/45/2020\""), "{error}");
    }

    #[test]
    fn a_retraction_marker_alone_does_not_retract_the_event() {
        let text = "AUTOMATIC REACTOR TRIP<br />\n<br />\n* * * RETRACTION ON 11/12/2026 AT 0900 EST FROM SAM HALL TO ADAM KOZIOL * * *<br />\n<br />\nNot reportable.";

        let event = one(Parts::reactor().text(text));

        assert!(!event.retracted);
        assert_eq!(event.updates.iter().map(summary).collect::<Vec<_>>(), ["Retraction 2026-11-12 09:00 EST | SAM HALL | ADAM KOZIOL"]);
    }
}
