//! The events explorer's filters (PRD-0004, DESIGN.md Decision 34), kept in the URL query as the
//! barcode's are, so every list is a permalink: `?from=2010-01-01&category=power-reactor&scram=1&q=fire`.
//! A parameter at its default is left out. The server parses the same query to answer `/api/events`.

use std::fmt::Write as _;

use chrono::{Datelike as _, NaiveDate};
use serde::{Deserialize, Serialize};

/// Events a page of the list holds.
pub const PAGE: usize = 50;

/// Series the counts chart draws: the seven largest groups and Other (the dataviz skill's cap).
pub const SERIES: usize = 8;

/// An event category as the explorer filters it: the NRC's published categories, with the two
/// pairs it renamed merged (General Information or Other became General Information, and
/// Non-Power Reactor became Research Reactor). Listed largest first, the counts chart's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    PowerReactor,
    AgreementState,
    GeneralInformation,
    FuelCycle,
    OtherNuclearMaterial,
    NonAgreementState,
    #[serde(rename = "part-21")]
    Part21,
    Hospital,
    ResearchReactor,
    SpentFuelStorage,
    Transportation,
}

impl Group {
    pub const ALL: [Self; 11] = [
        Self::PowerReactor,
        Self::AgreementState,
        Self::GeneralInformation,
        Self::FuelCycle,
        Self::OtherNuclearMaterial,
        Self::NonAgreementState,
        Self::Part21,
        Self::Hospital,
        Self::ResearchReactor,
        Self::SpentFuelStorage,
        Self::Transportation,
    ];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::PowerReactor => "power-reactor",
            Self::AgreementState => "agreement-state",
            Self::GeneralInformation => "general-information",
            Self::FuelCycle => "fuel-cycle",
            Self::OtherNuclearMaterial => "other-nuclear-material",
            Self::NonAgreementState => "non-agreement-state",
            Self::Part21 => "part-21",
            Self::Hospital => "hospital",
            Self::ResearchReactor => "research-reactor",
            Self::SpentFuelStorage => "spent-fuel-storage",
            Self::Transportation => "transportation",
        }
    }

    /// The name a reader sees: the newer of the pair's published labels.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PowerReactor => "Power Reactor",
            Self::AgreementState => "Agreement State",
            Self::GeneralInformation => "General Information",
            Self::FuelCycle => "Fuel Cycle Facility",
            Self::OtherNuclearMaterial => "Other Nuclear Material",
            Self::NonAgreementState => "Non-Agreement State",
            Self::Part21 => "Part 21",
            Self::Hospital => "Hospital",
            Self::ResearchReactor => "Research Reactor",
            Self::SpentFuelStorage => "Spent Fuel Storage",
            Self::Transportation => "Transportation",
        }
    }

    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.slug() == slug)
    }

    /// The counts chart's series: the seven largest groups each, and the rest as Other.
    #[must_use]
    pub fn series(self) -> usize {
        Self::ALL.iter().position(|group| *group == self).unwrap_or(SERIES - 1).min(SERIES - 1)
    }

    /// The group of a category as the event page labels it.
    #[must_use]
    pub fn of_label(label: &str) -> Option<Self> {
        Some(match label {
            "Power Reactor" => Self::PowerReactor,
            "Agreement State" => Self::AgreementState,
            "General Information" | "General Information or Other" => Self::GeneralInformation,
            "Fuel Cycle Facility" => Self::FuelCycle,
            "Other Nuclear Material" => Self::OtherNuclearMaterial,
            "Non-Agreement State" => Self::NonAgreementState,
            "Part 21" => Self::Part21,
            "Hospital" => Self::Hospital,
            "Research Reactor" | "Non-Power Reactor" => Self::ResearchReactor,
            "Independent Spent Fuel Storage Installation" => Self::SpentFuelStorage,
            "Transportation Event" => Self::Transportation,
            _ => return None,
        })
    }
}

/// An event's emergency class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Class {
    NonEmergency,
    UnusualEvent,
    Alert,
    SiteAreaEmergency,
    GeneralEmergency,
}

impl Class {
    pub const ALL: [Self; 5] = [Self::NonEmergency, Self::UnusualEvent, Self::Alert, Self::SiteAreaEmergency, Self::GeneralEmergency];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::NonEmergency => "non-emergency",
            Self::UnusualEvent => "unusual-event",
            Self::Alert => "alert",
            Self::SiteAreaEmergency => "site-area-emergency",
            Self::GeneralEmergency => "general-emergency",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NonEmergency => "Non Emergency",
            Self::UnusualEvent => "Unusual Event",
            Self::Alert => "Alert",
            Self::SiteAreaEmergency => "Site Area Emergency",
            Self::GeneralEmergency => "General Emergency",
        }
    }

    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.slug() == slug)
    }
}

/// What the explorer lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventsState {
    /// The first and last event date; `None` is open.
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// The groups listed; empty lists them all.
    pub groups: Vec<Group>,
    /// A two-letter state, upper case.
    pub state: Option<String>,
    /// Part of a facility's, site's or licensee's name.
    pub facility: Option<String>,
    /// A reactor unit's docket, e.g. `05000498`.
    pub unit: Option<String>,
    /// The start of a 10 CFR section, e.g. `50.72(b)(2)`.
    pub cfr: Option<String>,
    pub class: Option<Class>,
    /// Only events that scrammed a reactor.
    pub scram: bool,
    /// Words every listed event's title, text or names hold.
    pub q: Option<String>,
    /// From 1.
    pub page: usize,
}

impl Default for EventsState {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            groups: Vec::new(),
            state: None,
            facility: None,
            unit: None,
            cfr: None,
            class: None,
            scram: false,
            q: None,
            page: 1,
        }
    }
}

/// Text with its runs of whitespace collapsed, or `None` when nothing is left.
fn text(value: &str) -> Option<String> {
    let words: Vec<&str> = value.split_whitespace().collect();
    (!words.is_empty()).then(|| words.join(" "))
}

impl EventsState {
    /// The state a URL's query names, from a lookup of its decoded parameters. A parameter that
    /// doesn't parse keeps its default, so a mangled link still opens.
    #[must_use]
    pub fn from_query(param: impl Fn(&str) -> Option<String>) -> Self {
        let date = |name: &str| param(name).and_then(|value| value.parse().ok());
        let mut groups: Vec<Group> = param("category").map(|list| list.split(',').filter_map(Group::from_slug).collect()).unwrap_or_default();
        groups.sort();
        groups.dedup();
        Self {
            from: date("from"),
            to: date("to"),
            groups,
            state: param("state")
                .map(|state| state.trim().to_ascii_uppercase())
                .filter(|state| state.len() == 2 && state.bytes().all(|byte| byte.is_ascii_uppercase())),
            facility: param("facility").and_then(|facility| text(&facility)),
            unit: param("unit")
                .map(|unit| unit.trim().to_owned())
                .filter(|unit| unit.len() == 8 && unit.bytes().all(|byte| byte.is_ascii_digit())),
            cfr: param("cfr").and_then(|cfr| text(&cfr)),
            class: param("class").and_then(|class| Class::from_slug(&class)),
            scram: param("scram").as_deref() == Some("1"),
            q: param("q").and_then(|q| text(&q)),
            page: param("page").and_then(|page| page.parse().ok()).filter(|page| *page >= 1).unwrap_or(1),
        }
    }

    /// The state of a raw query string, with or without its `?`, decoding `+` and `%XX`.
    #[must_use]
    pub fn parse_query(query: &str) -> Self {
        let pairs: Vec<(String, String)> = query
            .trim_start_matches('?')
            .split('&')
            .filter_map(|pair| {
                let (name, value) = pair.split_once('=')?;
                Some((decode(name), decode(value)))
            })
            .collect();
        Self::from_query(|name| pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()))
    }

    /// The query string for this state, with its `?`, or empty at the defaults.
    #[must_use]
    pub fn to_query(&self) -> String {
        let mut params = Vec::new();
        let mut push = |name: &str, value: Option<String>| {
            if let Some(value) = value {
                params.push(format!("{name}={}", encode(&value)));
            }
        };
        push("from", self.from.map(|from| from.to_string()));
        push("to", self.to.map(|to| to.to_string()));
        push(
            "category",
            (!self.groups.is_empty()).then(|| self.groups.iter().map(|group| group.slug()).collect::<Vec<_>>().join(",")),
        );
        push("state", self.state.clone());
        push("facility", self.facility.clone());
        push("unit", self.unit.clone());
        push("cfr", self.cfr.clone());
        push("class", self.class.map(|class| class.slug().to_owned()));
        push("scram", self.scram.then(|| "1".to_owned()));
        push("q", self.q.clone());
        push("page", (self.page > 1).then(|| self.page.to_string()));
        if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) }
    }

    /// This state at `page`.
    #[must_use]
    pub fn at_page(&self, page: usize) -> Self {
        Self { page: page.max(1), ..self.clone() }
    }

    /// This state from its first page, as any change of filter starts.
    #[must_use]
    pub fn first_page(self) -> Self {
        Self { page: 1, ..self }
    }
}

/// One event as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRow {
    pub number: u32,
    /// The event date.
    pub date: NaiveDate,
    /// `None` for a category the explorer doesn't know.
    pub group: Option<Group>,
    /// The facility, site or licensee, as the catalogs name it where they do.
    pub place: String,
    pub state: Option<String>,
    /// A reactor event's units, by catalog name.
    pub units: Vec<String>,
    pub title: Option<String>,
    /// The 10 CFR sections' codes, as written.
    pub sections: Vec<String>,
    pub scram: bool,
    pub retracted: bool,
}

/// One page of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsPage {
    /// Events the filters select.
    pub total: usize,
    pub page: usize,
    pub pages: usize,
    pub rows: Vec<EventRow>,
}

/// Events per month by series ([`Group::series`]): the counts chart's data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCounts {
    /// The first day of the first month; `None` while empty.
    pub first: Option<NaiveDate>,
    pub months: Vec<[u64; SERIES]>,
}

impl EventCounts {
    /// Counts the events of each month from `first` through `last`, with no months when there are
    /// none. `events` gives each event's date and group.
    #[must_use]
    pub fn of(events: impl IntoIterator<Item = (NaiveDate, Option<Group>)>) -> Self {
        let events: Vec<(NaiveDate, Option<Group>)> = events.into_iter().collect();
        let month = |date: NaiveDate| i64::from(date.year()) * 12 + i64::from(date.month0());
        let Some(first) = events.iter().map(|(date, _)| *date).min() else {
            return Self::default();
        };
        let last = events.iter().map(|(date, _)| month(*date)).max().unwrap_or_default();
        let mut months = vec![[0; SERIES]; usize::try_from(last - month(first) + 1).unwrap_or_default()];
        for (date, group) in events {
            if let Some(counts) = usize::try_from(month(date) - month(first)).ok().and_then(|index| months.get_mut(index)) {
                counts[group.map_or(SERIES - 1, Group::series)] += 1;
            }
        }
        Self { first: first.with_day(1), months }
    }
}

/// Percent-encodes everything but the unreserved characters and the list's comma.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b',') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Decodes `+` and `%XX`; a malformed escape stays as written.
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' => match bytes
                .get(index + 1..index + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                Some(byte) => {
                    out.push(byte);
                    index += 2;
                }
                None => out.push(b'%'),
            },
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    #[test]
    fn uat_events_state_defaults_stay_out_of_the_url() {
        assert_eq!(EventsState::default().to_query(), "");
        assert_eq!(EventsState::parse_query(""), EventsState::default());
        assert_eq!(EventsState::parse_query("?"), EventsState::default());
        assert_eq!(EventsState::parse_query("page=1").to_query(), "", "the first page is the default");
    }

    #[test]
    fn uat_events_state_round_trips_every_filter() {
        let state = EventsState {
            from: Some(day("2010-01-01")),
            to: Some(day("2020-12-31")),
            groups: vec![Group::PowerReactor, Group::ResearchReactor],
            state: Some("TX".to_owned()),
            facility: Some("South Texas".to_owned()),
            unit: Some("05000498".to_owned()),
            cfr: Some("50.72(b)(2)(iv)(B)".to_owned()),
            class: Some(Class::UnusualEvent),
            scram: true,
            q: Some("diesel generator & fire".to_owned()),
            page: 3,
        };

        let query = state.to_query();

        assert_eq!(
            query,
            "?from=2010-01-01&to=2020-12-31&category=power-reactor,research-reactor&state=TX&facility=South%20Texas&unit=05000498&cfr=50.72%28b%29%282%29%28iv%29%28B%29&class=unusual-event&scram=1&q=diesel%20generator%20%26%20fire&page=3"
        );
        assert_eq!(EventsState::parse_query(&query), state);
    }

    #[test]
    fn uat_events_state_a_mangled_parameter_keeps_its_default() {
        let state = EventsState::parse_query("from=yesterday&category=power-reactor,zebras&state=Texas&unit=498&class=panic&scram=maybe&page=0&q=%20%20");

        assert_eq!(
            state,
            EventsState {
                groups: vec![Group::PowerReactor],
                ..EventsState::default()
            }
        );
    }

    #[test]
    fn uat_events_state_lowercase_states_and_spaced_text_normalize() {
        let state = EventsState::parse_query("state=tx&q=+Tritium++Leak+&facility=%20Vogtle%20");

        assert_eq!(
            (state.state.as_deref(), state.q.as_deref(), state.facility.as_deref()),
            (Some("TX"), Some("Tritium Leak"), Some("Vogtle"))
        );
    }

    #[test]
    fn uat_events_state_the_renamed_category_pairs_group() {
        assert_eq!(Group::of_label("General Information or Other"), Some(Group::GeneralInformation));
        assert_eq!(Group::of_label("General Information"), Some(Group::GeneralInformation));
        assert_eq!(Group::of_label("Research Reactor"), Some(Group::ResearchReactor));
        assert_eq!(Group::of_label("Non-Power Reactor"), Some(Group::ResearchReactor));
        assert_eq!(Group::of_label("Independent Spent Fuel Storage Installation"), Some(Group::SpentFuelStorage));
        assert_eq!(Group::of_label("Zebras"), None);
        assert_eq!(Group::ALL.len(), 11);
        assert!(Group::ALL.iter().all(|group| Group::from_slug(group.slug()) == Some(*group)));
        assert!(Class::ALL.iter().all(|class| Class::from_slug(class.slug()) == Some(*class)));
        let json = |value: serde_json::Value| value.as_str().map(str::to_owned);
        assert!(Group::ALL.iter().all(|group| json(serde_json::to_value(group).unwrap_or_default()).as_deref() == Some(group.slug())));
        assert!(Class::ALL.iter().all(|class| json(serde_json::to_value(class).unwrap_or_default()).as_deref() == Some(class.slug())));
    }

    #[test]
    fn the_eighth_series_is_other() {
        assert_eq!(Group::PowerReactor.series(), 0);
        assert_eq!(Group::Part21.series(), 6);
        assert_eq!([Group::Hospital, Group::ResearchReactor, Group::SpentFuelStorage, Group::Transportation].map(Group::series), [7; 4]);
    }

    #[test]
    fn uat_events_state_a_page_moves_and_a_filter_change_starts_over() {
        let state = EventsState::parse_query("q=fire&page=4");

        assert_eq!(state.at_page(5).to_query(), "?q=fire&page=5");
        assert_eq!(state.at_page(1).to_query(), "?q=fire");
        assert_eq!(EventsState { scram: true, ..state }.first_page().to_query(), "?scram=1&q=fire");
    }
}
