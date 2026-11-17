//! `See EN# N` resolution (DESIGN.md Decision 32, method `xref_en_v1`). A status day's reference
//! resolves when the archive holds event N, the event's unit table lists the day's unit, and the
//! event isn't retracted (Decision 30). The committed rule table in `catalog/xref.toml` then names
//! the cause from the unit's SCRAM Code or one of the event's 10 CFR sections, and an event no rule
//! matches takes the fallback. A reference that doesn't resolve keeps the reason why.

use anyhow::{Context as _, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::Res;
use crate::events::{EventSet, Scram, Section};
use crate::facilities::Identity;
use crate::tags::{Cause, Tags};

/// The rule table, embedded so the binary carries it.
pub(crate) const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/xref.toml"));

/// The field whose rule named a resolved reference's cause.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decider {
    /// The unit's SCRAM Code.
    Scram(Scram),
    /// A 10 CFR section the event lists, its code as written.
    Section(String),
}

/// A reference that resolved: the event, and the field that decided its cause, `None` when no rule
/// matched and the fallback did.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resolved {
    pub number: u32,
    pub decided_by: Option<Decider>,
}

/// Why a reference didn't resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The tag carries no number (`see en # pending`).
    NoNumber,
    /// The archive holds no such event.
    Missing,
    /// The event doesn't list the day's unit: another unit's event, or another category's, which
    /// is usually a typo in the number.
    OtherUnit,
    /// The NRC retracted the event, so it counts toward nothing (Decision 30).
    Retracted,
}

/// A reference that didn't resolve, with its number where the tag carried one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Unresolved {
    pub number: Option<u32>,
    pub reason: Reason,
}

/// One reference's outcome: the cause and what it rests on, or why there is none.
pub type Resolution<'a> = Result<(&'a Cause, Resolved), Unresolved>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    fallback: String,
    rules: Vec<RuleRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleRow {
    cause: String,
    #[serde(default)]
    scram: Vec<String>,
    #[serde(default)]
    sections: Vec<SectionRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionRow {
    code: String,
    description: Option<String>,
}

/// What a rule matches.
#[derive(Debug, Clone)]
enum Matcher {
    Scram(Vec<Scram>),
    Sections(Vec<SectionPattern>),
}

/// A section rule's code, without spaces, and its description, uppercased with whitespace collapsed.
#[derive(Debug, Clone)]
struct SectionPattern {
    code: String,
    description: Option<String>,
}

impl SectionPattern {
    /// Whether `section` falls under this pattern: its code is ours or continues it with a paragraph
    /// (`50.72(b)` covers `50.72(b)(2)(i)`, and `50.7` doesn't cover `50.72`), and its description
    /// matches ours when we have one.
    fn matches(&self, section: &Section) -> bool {
        let Some(code) = &section.code else { return false };
        let code: String = code.chars().filter(|char| !char.is_whitespace()).collect();
        let under = code.strip_prefix(self.code.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with('('));
        under && self.description.as_ref().is_none_or(|description| *description == words(&section.description))
    }
}

/// Uppercased, with whitespace runs collapsed to one space and the ends trimmed.
fn words(text: &str) -> String {
    text.split_whitespace().map(str::to_uppercase).collect::<Vec<_>>().join(" ")
}

/// The fine cause `name` from `tags`.
fn cause(tags: &Tags, name: &str) -> Res<Cause> {
    tags.cause(name).cloned().with_context(|| format!("{name:?} is not a fine cause in catalog/tags.toml"))
}

/// What a rule row matches on: exactly one of its SCRAM Codes and its sections.
fn matcher(rule: RuleRow) -> Res<Matcher> {
    Ok(match (rule.scram.is_empty(), rule.sections.is_empty()) {
        (false, true) => Matcher::Scram(rule.scram.iter().map(|code| Scram::parse(code)?.context("a blank SCRAM Code")).collect::<Res<_>>()?),
        (true, false) => Matcher::Sections(
            rule.sections
                .into_iter()
                .map(|section| {
                    let code: String = section.code.chars().filter(|char| !char.is_whitespace()).collect();
                    ensure!(!code.is_empty(), "a blank section code");
                    Ok(SectionPattern {
                        code,
                        description: section.description.as_deref().map(words),
                    })
                })
                .collect::<Res<_>>()?,
        ),
        _ => bail!("a rule matches on exactly one of `scram` and `sections`"),
    })
}

/// The rule table, each cause checked against the tag table's fine causes.
#[derive(Debug, Clone)]
pub struct Rules {
    rules: Vec<(Cause, Matcher)>,
    fallback: Cause,
}

impl Rules {
    /// The committed `catalog/xref.toml`, its causes checked against `tags`.
    ///
    /// # Errors
    ///
    /// When the table breaks a rule [`Rules::parse`] checks.
    pub fn catalog(tags: &Tags) -> Res<Self> {
        Self::parse(CATALOG, tags).context("catalog/xref.toml")
    }

    /// Parses a rule table.
    ///
    /// # Errors
    ///
    /// When the TOML doesn't parse, a cause isn't a fine cause in `tags`, a rule matches on neither
    /// or both of `scram` and `sections`, a SCRAM Code isn't one the pages use, or a section code is
    /// blank.
    pub fn parse(table: &str, tags: &Tags) -> Res<Self> {
        let file: File = toml::from_str(table).context("parsing the xref rules")?;
        let rules = file
            .rules
            .into_iter()
            .enumerate()
            .map(|(index, rule)| cause(tags, &rule.cause).and_then(|cause| Ok((cause, matcher(rule)?))).with_context(|| format!("rule {index}")))
            .collect::<Res<_>>()?;
        Ok(Self {
            rules,
            fallback: cause(tags, &file.fallback).context("the fallback")?,
        })
    }

    /// The cause for a unit whose SCRAM Code is `scram`, in an event listing `sections`, and the
    /// field that decided it: the first rule that matches, or the fallback with no field.
    #[must_use]
    pub fn decide(&self, scram: Option<Scram>, sections: &[Section]) -> (&Cause, Option<Decider>) {
        for (cause, matcher) in &self.rules {
            let decider = match matcher {
                Matcher::Scram(codes) => scram.filter(|scram| codes.contains(scram)).map(Decider::Scram),
                Matcher::Sections(patterns) => sections
                    .iter()
                    .find(|section| patterns.iter().any(|pattern| pattern.matches(section)))
                    .and_then(|section| section.code.clone())
                    .map(Decider::Section),
            };
            if decider.is_some() {
                return (cause, decider);
            }
        }
        (&self.fallback, None)
    }

    /// Resolves reference `number`, from a status day of the unit `docket`, against `events`.
    ///
    /// # Errors
    ///
    /// The reference's [`Unresolved`] when it has no number, the archive lacks the event, the event
    /// doesn't list the unit, or the event is retracted, checked in that order.
    pub fn resolve(&self, events: &EventSet, docket: &str, number: Option<u32>) -> Resolution<'_> {
        let unresolved = |reason| Unresolved { number, reason };
        let number = number.ok_or_else(|| unresolved(Reason::NoNumber))?;
        let event = events.events.get(&number).ok_or_else(|| unresolved(Reason::Missing))?;
        let Some(Identity::Reactor { dockets }) = events.identities.get(&number) else {
            return Err(unresolved(Reason::OtherUnit));
        };
        let unit = event
            .units
            .iter()
            .zip(dockets)
            .find_map(|(unit, unit_docket)| (unit_docket.as_str() == docket).then_some(unit))
            .ok_or_else(|| unresolved(Reason::OtherUnit))?;
        if event.retracted {
            return Err(unresolved(Reason::Retracted));
        }
        let (cause, decided_by) = self.decide(unit.scram, &event.sections);
        Ok((cause, Resolved { number, decided_by }))
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    fn section(code: &str, description: &str) -> Section {
        Section {
            code: Some(code.to_owned()),
            description: description.to_owned(),
        }
    }

    /// The cause name and deciding field for a unit with `scram` in an event listing `sections`.
    fn decide(rules: &Rules, scram: Option<Scram>, sections: &[Section]) -> (String, Option<Decider>) {
        let (cause, decider) = rules.decide(scram, sections);
        (cause.name.clone(), decider)
    }

    #[test]
    fn the_committed_table_parses() {
        Rules::catalog(&Tags::catalog().unwrap()).unwrap();
    }

    #[test]
    fn a_scram_code_other_than_n_is_a_scram_and_wins_over_a_section() {
        let rules = Rules::catalog(&Tags::catalog().unwrap()).unwrap();
        let shutdown = [section("50.72(b)(2)(i)", "PLANT S/D REQD BY TS")];

        assert_eq!(decide(&rules, Some(Scram::Auto), &shutdown), ("scram".to_owned(), Some(Decider::Scram(Scram::Auto))));
        assert_eq!(
            decide(&rules, Some(Scram::No), &shutdown),
            ("required_shutdown".to_owned(), Some(Decider::Section("50.72(b)(2)(i)".to_owned())))
        );
        assert_eq!(decide(&rules, None, &[]), ("en_reported".to_owned(), None));
        assert_eq!(decide(&rules, Some(Scram::No), &[]), ("en_reported".to_owned(), None));
    }

    #[test]
    fn a_required_shutdown_needs_its_description_in_either_era() {
        let rules = Rules::catalog(&Tags::catalog().unwrap()).unwrap();

        // Since 2001, written in title case from 2020 on, and with spaces in the code now and then.
        assert_eq!(
            decide(&rules, Some(Scram::No), &[section("50.72(b) (2) (i)", "Plant S/D Reqd By Ts")]),
            ("required_shutdown".to_owned(), Some(Decider::Section("50.72(b) (2) (i)".to_owned())))
        );
        // Before 2001, the same code was a degraded or unanalyzed condition.
        assert_eq!(decide(&rules, Some(Scram::No), &[section("50.72(b)(2)(i)", "DEG/UNANALYZED COND")]), ("en_reported".to_owned(), None));
    }

    #[test]
    fn a_section_code_matches_whole_numbers_and_paragraphs() {
        let tags = Tags::catalog().unwrap();
        let rules = Rules::parse(
            r#"
                fallback = "en_reported"
                [[rules]]
                cause = "required_shutdown"
                sections = [{ code = "50.7" }]
                [[rules]]
                cause = "scram"
                sections = [{ code = "50.72(b)" }]
            "#,
            &tags,
        )
        .unwrap();

        assert_eq!(decide(&rules, None, &[section("50.72(b)(2)(i)", "ANYTHING")]).0, "scram");
        assert_eq!(decide(&rules, None, &[section("50.72(b)", "ANYTHING")]).0, "scram");
        assert_eq!(decide(&rules, None, &[section("50.7(a)", "ANYTHING")]).0, "required_shutdown");
        assert_eq!(decide(&rules, None, &[section("50.73(a)(1)", "ANYTHING")]).0, "en_reported");
        assert_eq!(
            decide(
                &rules,
                None,
                &[Section {
                    code: None,
                    description: "AGREEMENT STATE".to_owned()
                }]
            )
            .0,
            "en_reported"
        );
    }

    #[test]
    fn the_table_rejects_unknown_causes_and_ambiguous_rules() {
        let tags = Tags::catalog().unwrap();
        let error = |table: &str| format!("{:#}", Rules::parse(table, &tags).unwrap_err());

        assert!(error("fallback = \"nope\"\nrules = []").contains("\"nope\" is not a fine cause"));
        assert!(error("fallback = \"en_reported\"\n[[rules]]\ncause = \"scram\"").contains("exactly one of"));
        assert!(error("fallback = \"en_reported\"\n[[rules]]\ncause = \"scram\"\nscram = [\"A/R\"]\nsections = [{ code = \"50.72\" }]").contains("exactly one of"));
        assert!(error("fallback = \"en_reported\"\n[[rules]]\ncause = \"scram\"\nscram = [\"X\"]").contains("scram code \"X\""));
        assert!(error("fallback = \"en_reported\"\n[[rules]]\ncause = \"scram\"\nscram = [\"\"]").contains("a blank SCRAM Code"));
        assert!(error("fallback = \"en_reported\"\n[[rules]]\ncause = \"scram\"\nsections = [{ code = \" \" }]").contains("a blank section code"));
    }
}
