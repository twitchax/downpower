//! Scram triggers (DESIGN.md Decisions 40 and 41, PRD-0007 T-002): what started each Event
//! Notification scram, read from its title and text by the ordered phrase rules in
//! `catalog/triggers.toml`, with a reviewed list that overrides them for named events. Never a
//! bare keyword: "loop 1" is a PWR's coolant loop, and the transformer the buses moved to after a
//! trip didn't cause it.

use std::collections::BTreeMap;

use anyhow::{Context as _, bail, ensure};
use regex::Regex;
use serde::Deserialize;

pub use downpower_core::triggers::{Circumstance, System, Tags};

use crate::Res;
use crate::events::{Event, EventSet, Scram};

/// The rules, embedded so the binary carries them.
pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/triggers.toml"));

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    near: usize,
    cause: String,
    cue: String,
    strip: Vec<String>,
    system: Vec<RuleRow<System>>,
    signal: Vec<RuleRow<System>>,
    circumstance: Vec<RuleRow<Circumstance>>,
    #[serde(default)]
    assign: Vec<Assign>,
}

/// A phrase rule as the file writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleRow<T> {
    value: T,
    pattern: String,
}

/// A reviewed override: the event, the tags it takes, and why.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assign {
    number: u32,
    system: Option<System>,
    circumstance: Option<Circumstance>,
    why: String,
}

/// One dimension's rules, in order: the first whose pattern matches gives the value.
#[derive(Debug, Clone)]
struct Rules<T>(Vec<(T, Regex)>);

impl<T: Copy + std::fmt::Debug> Rules<T> {
    fn compile(rows: Vec<RuleRow<T>>) -> Res<Self> {
        rows.into_iter()
            .map(|row| Ok((row.value, Regex::new(&row.pattern).with_context(|| format!("the pattern for {:?}", row.value))?)))
            .collect::<Res<_>>()
            .map(Self)
    }

    /// The first matching rule's index and value.
    fn first(&self, text: &str) -> Option<(usize, T)> {
        self.0.iter().enumerate().find(|(_, (_, pattern))| pattern.is_match(text)).map(|(index, (value, _))| (index, *value))
    }
}

/// What decided one dimension's tag, for the snapshot's review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decided {
    /// The rule at this index, from 0.
    Rule(usize),
    /// The signal rule at this index, from 0.
    Signal(usize),
    /// The reviewed list.
    Assigned,
    /// No rule matched: none named.
    Nothing,
}

/// The rules and the reviewed overrides.
#[derive(Debug, Clone)]
pub struct Triggers {
    near: usize,
    cause: Regex,
    cue: Regex,
    strip: Vec<Regex>,
    system: Rules<System>,
    signal: Rules<System>,
    circumstance: Rules<Circumstance>,
    assigned: BTreeMap<u32, Assign>,
}

impl Triggers {
    /// The committed rules.
    ///
    /// # Errors
    ///
    /// When `catalog/triggers.toml` breaks a rule [`Triggers::parse`] checks.
    pub fn catalog() -> Res<Self> {
        Self::parse(CATALOG).context("catalog/triggers.toml")
    }

    /// Parses a trigger catalog.
    ///
    /// # Errors
    ///
    /// When the TOML doesn't parse or names an unknown value, a pattern doesn't compile, or an
    /// override gives no reason, assigns nothing or names an event twice.
    pub fn parse(toml: &str) -> Res<Self> {
        let file: File = toml::from_str(toml).context("parsing the trigger catalog")?;
        let mut assigned = BTreeMap::new();
        for entry in file.assign {
            ensure!(!entry.why.trim().is_empty(), "EN {} is assigned without a reason", entry.number);
            ensure!(entry.system.is_some() || entry.circumstance.is_some(), "EN {} is assigned nothing", entry.number);
            let number = entry.number;
            ensure!(assigned.insert(number, entry).is_none(), "EN {number} is assigned twice");
        }
        let strip = file
            .strip
            .iter()
            .map(|pattern| Regex::new(pattern).with_context(|| format!("the strip pattern {pattern:?}")))
            .collect::<Res<_>>()?;
        Ok(Self {
            near: file.near,
            cause: Regex::new(&file.cause).context("the cause pattern")?,
            cue: Regex::new(&file.cue).context("the cue pattern")?,
            strip,
            system: Rules::compile(file.system)?,
            signal: Rules::compile(file.signal)?,
            circumstance: Rules::compile(file.circumstance)?,
            assigned,
        })
    }

    /// Checks the overrides against `events`: each names an unretracted event the archive holds
    /// that gives a unit a SCRAM Code other than `N`.
    ///
    /// # Errors
    ///
    /// On the first override that doesn't.
    pub fn check(&self, events: &EventSet) -> Res<()> {
        for number in self.assigned.keys() {
            let Some(event) = events.events.get(number) else {
                bail!("EN {number} is assigned but not in the archive")
            };
            ensure!(!event.retracted, "EN {number} is assigned but retracted");
            ensure!(
                event.units.iter().any(|unit| unit.scram.is_some_and(|scram| scram != Scram::No)),
                "EN {number} is assigned but scrammed no unit"
            );
        }
        Ok(())
    }

    /// The sentences the rules read: the title, then the text's sentences, updates included, each
    /// lowercased, every `strip` pattern blanked and whitespace collapsed; empty ones drop out.
    #[must_use]
    pub fn sentences(&self, event: &Event) -> Vec<String> {
        let body = format!("{}.\n{}", event.title.as_deref().unwrap_or_default(), event.text).to_lowercase();
        sentences(&body)
            .filter_map(|sentence| {
                let mut sentence = sentence.split_whitespace().collect::<Vec<_>>().join(" ");
                for pattern in &self.strip {
                    sentence = pattern.replace_all(&sentence, " ").into_owned();
                }
                let sentence = sentence.split_whitespace().collect::<Vec<_>>().join(" ");
                (sentence.len() > 1).then_some(sentence)
            })
            .collect()
    }

    /// The sentences joined, as the snapshot and the circumstance rules read them.
    #[must_use]
    pub fn excerpt(&self, event: &Event) -> String {
        self.sentences(event).join(" | ")
    }

    /// `event`'s tags: an override where the reviewed list has one, else the rules, else none
    /// named. See [`Triggers::system`] for the system; the circumstance is the first
    /// `circumstance` rule over the sentences `cue` matches, those about the trip.
    #[must_use]
    pub fn tags(&self, event: &Event) -> Tags {
        self.explain(event).0
    }

    /// The system the rules give `sentences`. The lead is the earliest sentence a `system` rule
    /// (a root cause) or a `signal` rule (only what tripped) matches, a root winning within it.
    /// A lead that is only a signal takes the first root a later sentence names within `near`
    /// sentences or with a word of cause (`cause`), `near` being how many sentences after the lead may
    /// name it without one. After that, only a later sentence naming a
    /// cause and a root further up the rule order changes it: "a turbine trip. The trip was caused
    /// by a loss of offsite power" is offsite power, while a pump that trips in the aftermath, told
    /// later and named as no cause, changes nothing.
    fn system(&self, sentences: &[String]) -> Option<(System, Decided)> {
        let (lead, mut found) = sentences.iter().enumerate().find_map(|(index, sentence)| {
            let found = match (self.system.first(sentence), self.signal.first(sentence)) {
                (Some((rule, system)), _) => (system, Decided::Rule(rule)),
                (None, Some((rule, system))) => (system, Decided::Signal(rule)),
                (None, None) => return None,
            };
            Some((index, found))
        })?;
        for (index, sentence) in sentences.iter().enumerate().skip(lead + 1) {
            let Some((rule, system)) = self.system.first(sentence) else { continue };
            let takes = match found.1 {
                Decided::Signal(_) => index <= lead + self.near || self.cause.is_match(sentence),
                Decided::Rule(current) => rule < current && self.cause.is_match(sentence),
                Decided::Assigned | Decided::Nothing => false,
            };
            if takes {
                found = (system, Decided::Rule(rule));
            }
        }
        Some(found)
    }

    /// `event`'s tags and what decided each.
    #[must_use]
    pub fn explain(&self, event: &Event) -> (Tags, [Decided; 2]) {
        let sentences = self.sentences(event);
        let assigned = self.assigned.get(&event.number);
        let none = Tags::default();
        let (system, by_system) = match assigned.and_then(|entry| entry.system) {
            Some(system) => (system, Decided::Assigned),
            None => self.system(&sentences).unwrap_or((none.system, Decided::Nothing)),
        };
        let text = sentences.iter().filter(|sentence| self.cue.is_match(sentence)).map(String::as_str).collect::<Vec<_>>().join(" | ");
        let (circumstance, by_circumstance) = match (assigned.and_then(|entry| entry.circumstance), self.circumstance.first(&text)) {
            (Some(circumstance), _) => (circumstance, Decided::Assigned),
            (None, Some((index, circumstance))) => (circumstance, Decided::Rule(index)),
            (None, None) => (none.circumstance, Decided::Nothing),
        };
        (Tags { system, circumstance }, [by_system, by_circumstance])
    }
}

/// `text`'s sentences: split after a period, semicolon, question or exclamation mark followed by
/// whitespace, or at a line break, so a decimal or an abbreviation without a space stays whole.
fn sentences(text: &str) -> impl Iterator<Item = &str> {
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut ends = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        let next_is_space = bytes.get(index + 1).is_some_and(u8::is_ascii_whitespace);
        if *byte == b'\n' || (matches!(byte, b'.' | b';' | b'?' | b'!') && next_is_space) {
            ends.push(index + 1);
        }
    }
    ends.push(text.len());
    ends.into_iter().filter_map(move |end| {
        let sentence = text.get(start..end)?;
        start = end;
        (!sentence.trim().is_empty()).then_some(sentence)
    })
}
