//! The observed layer's classifier (DESIGN.md Decisions 17, 18 and 21). A reason splits into tags,
//! and a tag is known through, in order, its exact key, the first keyword rule whose pattern matches
//! it, or the accepted-unknown list. All three live in committed files under `catalog/`, so every
//! label can be checked against the text it came from.

use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, anyhow, bail, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::Res;

/// Keys, rules, fine causes and flags.
const TAGS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/tags.toml"));
/// Tags accepted on purpose as Unattributed, one per line under a `tag` header.
const UNCLASSIFIED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/unclassified.tsv"));

/// The eight families, which are the barcode's colors (Decision 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Refueling,
    FuelCycle,
    Forced,
    Maintenance,
    Equipment,
    Environmental,
    Grid,
    Unattributed,
}

/// A fine cause, which keeps a cause addressable inside its family (Decision 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause {
    pub name: String,
    pub family: Family,
}

/// What a tag is (Decision 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label<'a> {
    /// Attributes lost output.
    Cause(&'a Cause),
    /// Context only, such as a grid or weather alert.
    Flag(&'a str),
    /// Increasing power, startup, holding: takes the unit's most recent cause.
    Transition,
    /// A reference to an Event Notification, with its number when the tag carries one.
    Xref(Option<u32>),
    /// Listed in `catalog/unclassified.tsv`: known on purpose, and counted as Unattributed.
    Unclassified,
}

/// How a tag was known, which the observed layer keeps as evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matched {
    Key,
    /// The rule's index in `catalog/tags.toml`, counting from 0.
    Rule(usize),
    Listed,
}

/// A known tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classified<'a> {
    pub label: Label<'a>,
    pub matched: Matched,
}

/// A label as the table spells it: `cause:<name>`, `flag:<name>`, `transition` or `xref`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spec {
    Cause(usize),
    Flag(usize),
    Transition,
    Xref,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    causes: HashMap<String, Family>,
    flags: Vec<String>,
    keys: HashMap<String, String>,
    rules: Vec<RuleRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleRow {
    pattern: String,
    label: String,
}

/// The tag table, with its rules compiled.
#[derive(Debug, Clone)]
pub struct Tags {
    causes: Vec<Cause>,
    flags: Vec<String>,
    keys: HashMap<String, Spec>,
    rules: Vec<Regex>,
    rule_labels: Vec<Spec>,
    unclassified: HashSet<String>,
    number: Regex,
}

impl Tags {
    /// The committed table and accepted-unknown list.
    ///
    /// # Errors
    ///
    /// When either file breaks a rule [`Tags::parse`] checks.
    pub fn catalog() -> Res<Self> {
        Self::parse(TAGS, UNCLASSIFIED).context("catalog/tags.toml or catalog/unclassified.tsv")
    }

    /// Parses a tag table and an accepted-unknown list.
    ///
    /// # Errors
    ///
    /// When the TOML doesn't parse, a label names a cause or flag the table doesn't define, a key
    /// or listed tag isn't normalized or appears twice, or a pattern doesn't compile.
    pub fn parse(table: &str, unclassified: &str) -> Res<Self> {
        let file: File = toml::from_str(table).context("parsing the tag table")?;

        let mut causes: Vec<Cause> = file.causes.into_iter().map(|(name, family)| Cause { name, family }).collect();
        causes.sort_by(|a, b| a.name.cmp(&b.name));
        let mut flags = file.flags;
        flags.sort();
        if let Some([flag, _]) = flags.windows(2).find(|pair| pair[0] == pair[1]) {
            bail!("flag {flag:?} appears twice");
        }
        let spec = |text: &str| -> Res<Spec> {
            match text.split_once(':') {
                Some(("cause", name)) => causes.iter().position(|cause| cause.name == name).map(Spec::Cause).ok_or_else(|| anyhow!("unknown cause {name:?}")),
                Some(("flag", name)) => flags.iter().position(|flag| flag == name).map(Spec::Flag).ok_or_else(|| anyhow!("unknown flag {name:?}")),
                None if text == "transition" => Ok(Spec::Transition),
                None if text == "xref" => Ok(Spec::Xref),
                _ => bail!("unknown label {text:?}"),
            }
        };

        let mut keys = HashMap::with_capacity(file.keys.len());
        for (key, label) in &file.keys {
            ensure!(normalize(key) == *key, "key {key:?} is not normalized");
            keys.insert(key.clone(), spec(label).with_context(|| format!("key {key:?}"))?);
        }
        let rule_labels = file
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| spec(&rule.label).with_context(|| format!("rule {index} ({:?})", rule.pattern)))
            .collect::<Res<Vec<_>>>()?;
        // Compiled one by one rather than as a set: only the first match matters, and a set has to
        // find them all, which ran about 6x slower over catalog/reasons.tsv.
        let rules = file
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| Regex::new(&rule.pattern).with_context(|| format!("compiling rule {index} ({:?})", rule.pattern)))
            .collect::<Res<Vec<_>>>()?;

        let mut lines = unclassified.lines();
        ensure!(lines.next() == Some("tag"), "the accepted-unknown list must start with a `tag` header");
        let mut listed = HashSet::new();
        for tag in lines {
            ensure!(normalize(tag) == tag && !tag.is_empty(), "listed tag {tag:?} is not normalized");
            ensure!(listed.insert(tag.to_owned()), "listed tag {tag:?} appears twice");
        }

        Ok(Self {
            causes,
            flags,
            keys,
            rules,
            rule_labels,
            unclassified: listed,
            number: Regex::new(r"\d+").context("the number pattern")?,
        })
    }

    /// Classifies one normalized tag (see [`tags`]); `None` means the tag is unknown.
    #[must_use]
    pub fn classify(&self, tag: &str) -> Option<Classified<'_>> {
        let (spec, matched) = if let Some(&spec) = self.keys.get(tag) {
            (spec, Matched::Key)
        } else if let Some(index) = self.rules.iter().position(|rule| rule.is_match(tag)) {
            (self.rule_labels[index], Matched::Rule(index))
        } else if self.unclassified.contains(tag) {
            return Some(Classified {
                label: Label::Unclassified,
                matched: Matched::Listed,
            });
        } else {
            return None;
        };
        let label = match spec {
            Spec::Cause(index) => Label::Cause(&self.causes[index]),
            Spec::Flag(index) => Label::Flag(&self.flags[index]),
            Spec::Transition => Label::Transition,
            Spec::Xref => Label::Xref(self.number.find(tag).and_then(|number| number.as_str().parse().ok())),
        };
        Some(Classified { label, matched })
    }

    /// Whether a key or rule knows `tag` without the accepted-unknown list.
    #[must_use]
    pub fn matches(&self, tag: &str) -> bool {
        self.keys.contains_key(tag) || self.rules.iter().any(|rule| rule.is_match(tag))
    }

    /// The pattern of rule `index`, counting from 0.
    #[must_use]
    pub fn rule_pattern(&self, index: usize) -> Option<&str> {
        self.rules.get(index).map(Regex::as_str)
    }

    /// Every tag on the accepted-unknown list.
    pub fn unclassified(&self) -> impl Iterator<Item = &str> {
        self.unclassified.iter().map(String::as_str)
    }
}

/// A tag as the table keys it: lowercase, with whitespace runs collapsed to one space and the ends
/// trimmed.
#[must_use]
pub fn normalize(tag: &str) -> String {
    let mut normal = String::with_capacity(tag.len());
    for word in tag.split_whitespace() {
        if !normal.is_empty() {
            normal.push(' ');
        }
        normal.extend(word.chars().flat_map(char::to_lowercase));
    }
    normal
}

/// The normalized tags in a reason, which separates them with `;` or line breaks.
pub fn tags(reason: &str) -> impl Iterator<Item = String> + '_ {
    reason.split([';', '\n']).map(normalize).filter(|tag| !tag.is_empty())
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    const TABLE: &str = r#"
        flags = ["hot_weather"]
        [causes]
        refueling = "refueling"
        condenser = "equipment"
        [keys]
        "refueling outage" = "cause:refueling"
        "hot weather alert" = "flag:hot_weather"
        "increasing power" = "transition"
        [[rules]]
        pattern = '\brefuel'
        label = "cause:refueling"
        [[rules]]
        pattern = 'condenser'
        label = "cause:condenser"
        [[rules]]
        pattern = '\ben ?#'
        label = "xref"
    "#;

    fn table(unclassified: &str) -> Tags {
        Tags::parse(TABLE, &format!("tag\n{unclassified}")).unwrap()
    }

    fn error(table: &str, unclassified: &str) -> String {
        format!("{:#}", Tags::parse(table, unclassified).unwrap_err())
    }

    #[test]
    fn reasons_split_on_semicolons_and_line_breaks() {
        let tags: Vec<_> = tags("Steam Generator Limitations;\n\nMaximum  Generation Alert\n").collect();

        assert_eq!(tags, ["steam generator limitations", "maximum generation alert"]);
        assert_eq!(normalize("  REFUELING\tOUTAGE - 413°F "), "refueling outage - 413°f");
    }

    #[test]
    fn keys_come_before_rules_and_rules_before_the_list() {
        let tags = table("loading fuel\n");
        let refueling = Label::Cause(&tags.causes[1]);

        assert_eq!(tags.classify("refueling outage"), Some(Classified { label: refueling, matched: Matched::Key }));
        assert_eq!(
            tags.classify("refueling outage - defueled"),
            Some(Classified {
                label: refueling,
                matched: Matched::Rule(0)
            })
        );
        assert_eq!(tags.classify("hot weather alert").unwrap().label, Label::Flag("hot_weather"));
        assert_eq!(tags.classify("increasing power").unwrap().label, Label::Transition);
        assert_eq!(
            tags.classify("loading fuel"),
            Some(Classified {
                label: Label::Unclassified,
                matched: Matched::Listed
            })
        );
        assert_eq!(tags.classify("fish in the intake"), None);
        assert!(!tags.matches("loading fuel"));
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let tags = table("");

        let classified = tags.classify("refuel delayed by condenser leak").unwrap();

        assert_eq!(classified.matched, Matched::Rule(0));
        assert_eq!(tags.classify("condenser tube leak").unwrap().matched, Matched::Rule(1));
    }

    #[test]
    fn an_xref_keeps_its_event_number() {
        let tags = table("");

        assert_eq!(tags.classify("see en #49154").unwrap().label, Label::Xref(Some(49154)));
        assert_eq!(tags.classify("see en # pending").unwrap().label, Label::Xref(None));
    }

    #[test]
    fn the_table_is_checked() {
        let header = "tag\n";
        let cases = [
            (
                TABLE.replace(r#""refueling outage" = "cause:refueling""#, r#""Refueling Outage" = "cause:refueling""#),
                header,
                "key \"Refueling Outage\" is not normalized",
            ),
            (TABLE.replace(r#"label = "cause:condenser""#, r#"label = "cause:turbine""#), header, "unknown cause \"turbine\""),
            (TABLE.replace(r#""flag:hot_weather""#, r#""flag:heat""#), header, "unknown flag \"heat\""),
            (TABLE.replace(r#""transition""#, r#""shift""#), header, "unknown label \"shift\""),
            (TABLE.replace(r"'\brefuel'", "'('"), header, "compiling rule 0"),
            (TABLE.replace(r#"flags = ["hot_weather"]"#, r#"flags = ["hot_weather", "hot_weather"]"#), header, "appears twice"),
            (TABLE.to_owned(), "tags\n", "must start with a `tag` header"),
            (TABLE.to_owned(), "tag\nLoading Fuel\n", "is not normalized"),
            (TABLE.to_owned(), "tag\nloading fuel\nloading fuel\n", "appears twice"),
        ];
        for (table, unclassified, expected) in cases {
            let error = error(&table, unclassified);
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }
}
