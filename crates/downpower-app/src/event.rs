//! `/events/{number}`: one Event Notification in full (PRD-0004 T-005): its header, a reactor
//! event's unit table and each unit's barcode strip around the date, the update timeline, the text,
//! and the NRC page it came from. The types mirror the subset of `/api/events/{number}` shown here.

use std::sync::Arc;

use chrono::{Days, NaiveDate, NaiveTime};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;

use downpower_core::barcode::{Barcode, Cell, FULL};
use downpower_core::explorer::{Class, EventsState};
use downpower_core::triggers::{Circumstance, Tags};

/// Days the strip shows either side of the event.
pub const STRIP_DAYS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Detail {
    pub number: u32,
    pub retracted: bool,
    pub site: Site,
    pub state: Option<String>,
    pub notified_by: Option<String>,
    pub ops_officer: Option<String>,
    pub notified: Stamp,
    pub occurred: Stamp,
    pub updated: Option<NaiveDate>,
    pub class: Option<String>,
    pub sections: Vec<Section>,
    pub persons: Vec<String>,
    pub units: Vec<UnitInfo>,
    pub title: Option<String>,
    pub text: String,
    pub updates: Vec<Update>,
    pub revised: Option<NaiveDate>,
    pub identity: Option<Identity>,
    pub nrc: Option<String>,
    pub label: String,
    pub unit_names: Vec<String>,
    /// An unretracted scram's system and circumstance (PRD-0007 T-007).
    #[serde(default)]
    pub trigger: Option<Tags>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    Facility { name: String },
    Licensee { rep_org: Option<String>, name: Option<String>, city: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Stamp {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Section {
    pub code: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UnitInfo {
    pub unit: u8,
    pub scram: Option<String>,
    pub critical: bool,
    pub initial_power: u8,
    pub initial_mode: Option<String>,
    pub current_power: u8,
    pub current_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Update {
    pub kind: String,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub zone: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Identity {
    Reactor { dockets: Vec<String> },
    Facility { docket: String, name: String },
    Licensee { state: String, license: String, name: Option<String> },
}

impl Detail {
    /// The facility, site or licensee, as the catalogs name it where they do.
    #[must_use]
    pub fn place(&self) -> String {
        match (&self.identity, &self.site) {
            (Some(Identity::Facility { name, .. } | Identity::Licensee { name: Some(name), .. }), _) | (_, Site::Facility { name }) => name.clone(),
            (_, Site::Licensee { name, rep_org, .. }) => name.clone().or_else(|| rep_org.clone()).unwrap_or_default(),
        }
    }

    /// A reactor event's units: each docket and its name.
    #[must_use]
    pub fn reactor_units(&self) -> Vec<(String, String)> {
        match &self.identity {
            Some(Identity::Reactor { dockets }) => dockets.iter().cloned().zip(self.unit_names.iter().cloned()).collect(),
            _ => Vec::new(),
        }
    }
}

/// A SCRAM Code as the NRC writes it.
#[must_use]
pub fn scram_code(scram: Option<&str>) -> &'static str {
    match scram {
        Some("no") => "N",
        Some("auto_reactor") => "A/R",
        Some("manual_reactor") => "M/R",
        Some("auto") => "A",
        Some("manual") => "M",
        _ => "",
    }
}

/// A scram's tags as the facts list writes them: the system, and what was going on when the text
/// names it.
#[must_use]
pub fn trigger_label(tags: Tags) -> String {
    let during = match tags.circumstance {
        Circumstance::Weather => ", weather",
        Circumstance::Testing => ", during testing",
        Circumstance::Maintenance => ", during maintenance",
        Circumstance::HumanError => ", an operator or technician error",
        Circumstance::Unnamed => "",
    };
    format!("{}{during}", tags.system.label())
}

/// An emergency class's label.
#[must_use]
pub fn class_label(class: &str) -> &'static str {
    Class::from_slug(&class.replace('_', "-")).map_or("", Class::label)
}

/// An update marker's kind in words.
#[must_use]
pub fn update_kind(kind: &str) -> &'static str {
    match kind {
        "update" => "Update",
        "retraction" => "Retraction",
        "partial_retraction" => "Partial retraction",
        "unretraction" => "Retraction withdrawn",
        _ => "Note",
    }
}

/// `docket`'s cells from [`STRIP_DAYS`] before `date` through as many after, each day `None` where
/// the barcode has no row, no day, or no data.
#[must_use]
pub fn strip(barcode: &Barcode, docket: &str, date: NaiveDate) -> Vec<(NaiveDate, Option<Cell>)> {
    let row = barcode.units().iter().position(|unit| unit.docket == docket);
    let first = date.checked_sub_days(Days::new(STRIP_DAYS)).unwrap_or(date);
    first
        .iter_days()
        .take(usize::try_from(2 * STRIP_DAYS + 1).unwrap_or_default())
        .map(|day| (day, row.and_then(|row| barcode.cell(row, barcode.index(day)?))))
        .collect()
}

fn stamp(stamp: &Stamp) -> String {
    format!(
        "{} {}{}",
        stamp.date,
        stamp.time.format("%H:%M"),
        stamp.zone.as_ref().map(|zone| format!(" {zone}")).unwrap_or_default()
    )
}

#[cfg(feature = "hydrate")]
async fn load<T: serde::de::DeserializeOwned>(url: String) -> Result<Option<Arc<T>>, String> {
    let Some(bytes) = crate::browser::fetch(&url).await? else { return Ok(None) };
    serde_json::from_slice(&bytes).map(|value| Some(Arc::new(value))).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load<T>(_url: String) -> Result<Option<Arc<T>>, String> {
    Err("the event loads in the browser".to_owned())
}

#[component]
pub fn EventPage() -> impl IntoView {
    let params = use_params_map();
    let number = Memo::new(move |_| params.with(|params| params.get("number")).unwrap_or_default());
    let data = LocalResource::new(move || load::<Detail>(format!("/api/events/{}", number.get())));
    move || match data.get() {
        None => view! { <p class="meta">"loading…"</p> }.into_any(),
        Some(Err(error)) => view! { <p class="meta">{error}</p> }.into_any(),
        Some(Ok(None)) => view! {
            <header class="intro">
                <h1>{format!("No event {}", number.get())}</h1>
                <p class="muted"><a href="/events">"Every event"</a></p>
            </header>
        }
        .into_any(),
        Some(Ok(Some(detail))) => view! { <Event detail=(*detail).clone() /> }.into_any(),
    }
}

#[component]
fn Event(detail: Detail) -> impl IntoView {
    let title = detail.title.clone().unwrap_or_else(|| format!("Event {}", detail.number));
    let units = detail.reactor_units();
    let mut facts = vec![
        ("Event", stamp(&detail.occurred)),
        ("Notified", stamp(&detail.notified)),
        ("Category", detail.label.clone()),
        ("Place", format!("{}{}", detail.place(), detail.state.as_ref().map(|state| format!(", {state}")).unwrap_or_default())),
    ];
    facts.extend(detail.trigger.map(|tags| ("Scram trigger", trigger_label(tags))));
    facts.extend(detail.class.as_deref().map(|class| ("Emergency class", class_label(class).to_owned())));
    facts.extend(detail.updated.map(|updated| ("Last update", updated.to_string())));
    facts.extend(detail.revised.map(|revised| ("Revision imported", revised.to_string())));
    facts.extend(detail.notified_by.clone().map(|by| ("Notified by", by)));
    facts.extend(detail.ops_officer.clone().map(|officer| ("HQ ops officer", officer)));
    let facts = facts.into_iter().map(|(label, value)| view! { <dt class="label">{label}</dt><dd>{value}</dd> }).collect_view();
    let sections = detail
        .sections
        .iter()
        .map(|section| {
            let code = section.code.clone().unwrap_or_default();
            let href = (!code.is_empty()).then(|| {
                format!(
                    "/events{}",
                    EventsState {
                        cfr: Some(code.clone()),
                        ..EventsState::default()
                    }
                    .to_query()
                )
            });
            view! { <li><a href=href>{code}</a> " " <span class="meta">{section.description.clone()}</span></li> }
        })
        .collect_view();
    let unit_rows = detail
        .units
        .iter()
        .zip(units.iter().map(|(_, name)| name.clone()).chain(std::iter::repeat(String::new())))
        .map(|(unit, name)| {
            view! {
                <tr>
                    <th scope="row">{if name.is_empty() { format!("Unit {}", unit.unit) } else { name }}</th>
                    <td>{scram_code(unit.scram.as_deref())}</td>
                    <td>{if unit.critical { "Y" } else { "N" }}</td>
                    <td>{format!("{}%", unit.initial_power)} " " <span class="meta">{unit.initial_mode.clone()}</span></td>
                    <td>{format!("{}%", unit.current_power)} " " <span class="meta">{unit.current_mode.clone()}</span></td>
                </tr>
            }
        })
        .collect_view();
    let has_units = !detail.units.is_empty();
    let updates = detail
        .updates
        .iter()
        .map(|update| {
            let when = match (update.date, update.time) {
                (Some(date), Some(time)) => format!("{date} {}{}", time.format("%H:%M"), update.zone.as_ref().map(|zone| format!(" {zone}")).unwrap_or_default()),
                (Some(date), None) => date.to_string(),
                _ => "undated".to_owned(),
            };
            let who = match (&update.from, &update.to) {
                (Some(from), Some(to)) => format!("{from} to {to}"),
                (Some(from), None) => from.clone(),
                (None, Some(to)) => format!("to {to}"),
                (None, None) => String::new(),
            };
            view! { <li><span class="state">{update_kind(&update.kind)}</span> " " <span class="meta">{when}</span> " " <span class="meta">{who}</span></li> }
        })
        .collect_view();
    let has_updates = !detail.updates.is_empty();
    let date = detail.occurred.date;
    let strips = units.into_iter().map(|(docket, name)| view! { <Strip docket name date /> }).collect_view();
    let more = units_link(&detail);
    view! {
        <header class="intro">
            <p class="meta">
                <a href="/events">"Events"</a>
                {format!(" / EN {}", detail.number)}
            </p>
            <h1 class:struck=detail.retracted>{title}</h1>
            {detail.retracted.then(|| view! { <p><span class="chip">"retracted"</span> " " <span class="muted">"The NRC withdrew this event; it counts toward nothing."</span></p> })}
        </header>
        <section class="panel">
            <dl class="facts">{facts}</dl>
            {(!detail.sections.is_empty()).then(|| view! { <div class="label">"10 CFR"</div> <ul class="plain">{sections}</ul> })}
            {has_units.then(|| view! {
                <table class="totals">
                    <caption class="meta">"Power reactor unit info"</caption>
                    <thead><tr><th scope="col">"Unit"</th><th scope="col">"SCRAM"</th><th scope="col">"Critical"</th><th scope="col">"Initial"</th><th scope="col">"Current"</th></tr></thead>
                    <tbody>{unit_rows}</tbody>
                </table>
            })}
            {strips}
            <p class="row">
                {detail.nrc.clone().map(|nrc| view! { <a class="btn" href=nrc>"On nrc.gov"</a> })}
                {more}
            </p>
        </section>
        {has_updates.then(|| view! {
            <section class="panel">
                <div class="label">"Updates"</div>
                <ol class="plain">{updates}</ol>
            </section>
        })}
        <section class="panel">
            <div class="label">"Event text"</div>
            <p class="event-text">{detail.text.clone()}</p>
        </section>
    }
}

/// A link to the explorer filtered to this event's units, when it names any.
fn units_link(detail: &Detail) -> Option<impl IntoView + use<>> {
    let (docket, name) = detail.reactor_units().into_iter().next()?;
    let href = format!(
        "/events{}",
        EventsState {
            unit: Some(docket),
            ..EventsState::default()
        }
        .to_query()
    );
    Some(view! { <a class="btn" href=href>{format!("More from {name}")}</a> })
}

#[cfg(feature = "hydrate")]
async fn load_barcode() -> Result<Arc<Barcode>, String> {
    let bytes = crate::browser::fetch(&format!("/api/barcode/{}", downpower_core::barcode::Layer::Attributed.slug()))
        .await?
        .ok_or("no barcode yet")?;
    Barcode::decode(&bytes).map(Arc::new).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load_barcode() -> Result<Arc<Barcode>, String> {
    Err("the barcode loads in the browser".to_owned())
}

/// One unit's attributed barcode row around the event: a bar per day as tall as the power it
/// lost, in its cause family's color, with the event's day marked.
#[component]
fn Strip(docket: String, name: String, date: NaiveDate) -> impl IntoView {
    let data = LocalResource::new(load_barcode);
    let href = format!(
        "/?from={}&to={}",
        date.checked_sub_days(Days::new(STRIP_DAYS)).unwrap_or(date),
        date.checked_add_days(Days::new(STRIP_DAYS)).unwrap_or(date)
    );
    let caption = format!("{name}, {STRIP_DAYS} days either side");
    let bars = move || {
        let barcode = data.get()?.ok()?;
        let days = strip(&barcode, &docket, date);
        let bars = days
            .iter()
            .enumerate()
            .filter_map(|(index, (_, cell))| {
                let cell = (*cell)?;
                let lost = f64::from(FULL.saturating_sub(cell.bucket)) / f64::from(FULL);
                (lost > 0.0).then(|| view! { <rect class=format!("family-{}", cell.family.slug()) x=index y=format!("{:.2}", 1.0 - lost) width="1" height=format!("{lost:.2}")></rect> })
            })
            .collect_view();
        Some(view! {
            {bars}
            <path class="crosshair" d=format!("M{}.5,0V1", STRIP_DAYS)></path>
        })
    };
    view! {
        <figure class="strip">
            <figcaption class="meta"><a href=href>{caption}</a></figcaption>
            <svg viewBox=format!("0 0 {} 1", 2 * STRIP_DAYS + 1) preserveAspectRatio="none" role="img" aria-label="The unit's lost power by day around the event, colored by cause family">
                {bars}
            </svg>
        </figure>
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use downpower_core::Family;
    use downpower_core::barcode::Unit;

    const DETAIL: &str = r#"{
        "number": 45290, "category": "power_reactor", "retracted": false,
        "site": {"facility": {"name": "BROWNS FERRY", "units": [3], "rx_type": null, "comments": null}},
        "region": 2, "state": "AL", "notified_by": "SMITH", "ops_officer": "JONES",
        "notified": {"date": "2009-08-24", "time": "20:30:00", "zone": "ET"},
        "occurred": {"date": "2009-08-24", "time": "18:50:00", "zone": "CDT"},
        "updated": "2009-08-24", "class": "non_emergency",
        "sections": [{"code": "50.72(b)(2)(iv)(B)", "description": "RPS ACTUATION - CRITICAL"}],
        "persons": [], "title": "CONDENSATE BOOSTER PUMP TRIP", "text": "The unit scrammed.",
        "units": [{"unit": 3, "scram": "manual_reactor", "critical": true, "initial_power": 100, "initial_mode": "Power Operation", "current_power": 0, "current_mode": "Hot Standby"}],
        "updates": [{"kind": "update", "marker": "UPDATE", "date": "2009-08-25", "time": null, "zone": null, "from": "A", "to": "B", "text": ""}],
        "revised": null, "identity": {"kind": "reactor", "dockets": ["05000296"]}, "page": "2009-08-24",
        "nrc": "https://www.nrc.gov/x#en45290", "label": "Power Reactor", "unit_names": ["Browns Ferry 3"],
        "trigger": {"system": "feedwater", "circumstance": "testing"}
    }"#;

    #[test]
    fn the_event_route_parses_into_the_page_types() {
        let detail: Detail = serde_json::from_str(DETAIL).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(detail.place(), "BROWNS FERRY");
        assert_eq!(detail.reactor_units(), [("05000296".to_owned(), "Browns Ferry 3".to_owned())]);
        assert_eq!(scram_code(detail.units[0].scram.as_deref()), "M/R");
        assert_eq!(class_label(detail.class.as_deref().unwrap_or_default()), "Non Emergency");
        assert_eq!(update_kind(&detail.updates[0].kind), "Update");
        assert_eq!(detail.trigger.map(trigger_label).as_deref(), Some("Feedwater and condensate, during testing"));
    }

    #[test]
    fn a_strip_is_the_units_days_around_the_event() {
        let date = |text: &str| text.parse::<NaiveDate>().unwrap_or_default();
        let units = vec![Unit {
            docket: "05000296".to_owned(),
            name: "Browns Ferry 3".to_owned(),
        }];
        let mut cells = vec![Cell::new(Family::Unattributed, 100).pack(); 3];
        cells[1] = Cell::new(Family::Forced, 0).pack();
        let barcode = Barcode::new(date("2009-08-23"), 3, units, cells).unwrap_or_else(|error| panic!("{error}"));

        let days = strip(&barcode, "05000296", date("2009-08-24"));

        assert_eq!(days.len(), 121);
        assert_eq!(days[60], (date("2009-08-24"), Some(Cell::new(Family::Forced, 0))));
        assert_eq!(days[59].1, Some(Cell::new(Family::Unattributed, 100)));
        assert_eq!(days[0].1, None, "before the barcode starts");
        assert!(strip(&barcode, "05000999", date("2009-08-24")).iter().all(|(_, cell)| cell.is_none()));
    }
}
