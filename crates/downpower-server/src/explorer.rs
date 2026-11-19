//! The events explorer's working copy of the serving build's events (PRD-0004 T-003): each event's
//! list row beside what its filters read, with its title, text and names lowercased for search.
//! Search scans that text rather than a full-text index, which measured no faster at four times the
//! memory (DESIGN.md Decision 34, PRD-0004.log.md#t-001).

use std::collections::HashMap;

use downpower_core::explorer::{Class, EventCounts, EventRow, EventsPage, EventsState, Group};
use downpower_ingest::events::{EmergencyClass, Event, Scram, Site};
use downpower_ingest::facilities::Identity;
use downpower_ingest::ffd::Ffd;
use downpower_ingest::units::Units;

use crate::ledger::StoredEvent;

/// One event: its row, and what the filters read.
#[derive(Debug)]
struct Entry {
    row: EventRow,
    class: Option<Class>,
    /// A reactor event's unit dockets, or a facility's docket.
    dockets: Vec<String>,
    /// The 10 CFR codes without their spaces, upper case, as the `cfr` filter compares them.
    sections: Vec<String>,
    /// The facility, site, licensee and unit names, lowercased.
    names: String,
    /// The title, text, names and persons, lowercased.
    text: String,
    /// A fitness-for-duty event, as `catalog/ffd.toml` identifies one.
    ffd: bool,
}

/// Every event of one build, newest first.
#[derive(Debug)]
pub struct Explorer {
    key: String,
    entries: Vec<Entry>,
}

/// A 10 CFR code as the filter compares it: `50.72(b) (2)` and `50.72(b)(2)` alike.
fn section_key(code: &str) -> String {
    code.split_whitespace().collect::<String>().to_ascii_uppercase()
}

const fn class(class: EmergencyClass) -> Class {
    match class {
        EmergencyClass::NonEmergency => Class::NonEmergency,
        EmergencyClass::UnusualEvent => Class::UnusualEvent,
        EmergencyClass::Alert => Class::Alert,
        EmergencyClass::SiteAreaEmergency => Class::SiteAreaEmergency,
        EmergencyClass::GeneralEmergency => Class::GeneralEmergency,
    }
}

fn entry(event: Event, identity: Option<&Identity>, unit_names: &HashMap<&str, &str>, ffd: &Ffd) -> Entry {
    let is_ffd = ffd.identify(&event).is_some();
    let dockets: Vec<String> = match identity {
        Some(Identity::Reactor { dockets }) => dockets.iter().map(|docket| docket.as_str().to_owned()).collect(),
        Some(Identity::Facility { docket, .. }) => vec![docket.as_str().to_owned()],
        Some(Identity::Licensee { .. }) | None => Vec::new(),
    };
    let units: Vec<String> = match identity {
        Some(Identity::Reactor { .. }) => dockets.iter().filter_map(|docket| unit_names.get(docket.as_str()).map(|name| (*name).to_owned())).collect(),
        _ => Vec::new(),
    };
    let (site, rep_org) = match &event.site {
        Site::Facility(facility) => (Some(facility.name.as_str()), None),
        Site::Licensee(licensee) => (licensee.name.as_deref(), licensee.rep_org.as_deref()),
    };
    let place = match identity {
        Some(Identity::Facility { name, .. } | Identity::Licensee { name: Some(name), .. }) => name.clone(),
        _ => site.or(rep_org).unwrap_or_default().to_owned(),
    };
    let names = [Some(place.as_str()), site, rep_org]
        .into_iter()
        .flatten()
        .chain(units.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let text = [event.title.as_deref().unwrap_or_default(), &event.text, &names, &event.persons.join("\n")].join("\n").to_lowercase();
    Entry {
        class: event.class.map(class),
        sections: event.sections.iter().filter_map(|section| section.code.as_deref()).map(section_key).collect(),
        row: EventRow {
            number: event.number,
            date: event.occurred.date,
            group: Group::of_label(event.category.label()),
            place,
            state: event.state,
            units,
            title: event.title,
            sections: event.sections.into_iter().filter_map(|section| section.code).collect(),
            scram: event.units.iter().any(|unit| unit.scram.is_some_and(|scram| scram != Scram::No)),
            retracted: event.retracted,
        },
        dockets,
        names,
        text,
        ffd: is_ffd,
    }
}

impl Explorer {
    /// Indexes `events` under `key`, which names the build they came from.
    #[must_use]
    pub fn new(key: impl Into<String>, events: impl IntoIterator<Item = StoredEvent>, units: &Units, ffd: &Ffd) -> Self {
        let unit_names: HashMap<&str, &str> = units.all().iter().map(|unit| (unit.docket.as_str(), unit.name.as_str())).collect();
        let mut entries: Vec<Entry> = events.into_iter().map(|stored| entry(stored.event, stored.identity.as_ref(), &unit_names, ffd)).collect();
        entries.sort_by_key(|entry| std::cmp::Reverse((entry.row.date, entry.row.number)));
        Self { key: key.into(), entries }
    }

    /// The build the events came from.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The events `state` selects, newest first, the event date then the number breaking ties.
    fn selected<'a>(&'a self, state: &'a EventsState) -> impl Iterator<Item = &'a Entry> {
        let facility = state.facility.as_deref().map(str::to_lowercase);
        let cfr = state.cfr.as_deref().map(section_key);
        let words: Vec<String> = state.q.as_deref().map(|q| q.to_lowercase().split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
        self.entries.iter().filter(move |entry| {
            let row = &entry.row;
            state.from.is_none_or(|from| row.date >= from)
                && state.to.is_none_or(|to| row.date <= to)
                && (state.groups.is_empty() || row.group.is_some_and(|group| state.groups.contains(&group)))
                && state.state.as_ref().is_none_or(|wanted| row.state.as_ref() == Some(wanted))
                && facility.as_deref().is_none_or(|facility| entry.names.contains(facility))
                && state.unit.as_ref().is_none_or(|unit| entry.dockets.contains(unit))
                && cfr.as_deref().is_none_or(|cfr| entry.sections.iter().any(|section| section.starts_with(cfr)))
                && state.class.is_none_or(|class| entry.class == Some(class))
                && (!state.scram || row.scram)
                && (!state.ffd || entry.ffd)
                && words.iter().all(|word| entry.text.contains(word.as_str()))
        })
    }

    /// Page `state.page` of the events `state` selects, `per` to a page.
    #[must_use]
    pub fn page(&self, state: &EventsState, per: usize) -> EventsPage {
        let per = per.max(1);
        let selected: Vec<&Entry> = self.selected(state).collect();
        let rows = selected.iter().skip((state.page.max(1) - 1).saturating_mul(per)).take(per).map(|entry| entry.row.clone()).collect();
        EventsPage {
            total: selected.len(),
            page: state.page,
            pages: selected.len().div_ceil(per).max(1),
            rows,
        }
    }

    /// The events `state` selects, counted by month and series.
    #[must_use]
    pub fn counts(&self, state: &EventsState) -> EventCounts {
        EventCounts::of(self.selected(state).map(|entry| (entry.row.date, entry.row.group)))
    }
}
