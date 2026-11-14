//! The two label layers (DESIGN.md Decisions 18–20). [`observe`] turns a reason into what its tags
//! say, which is checkable against the text. [`attribute`] infers one primary cause per unit-day
//! from a unit's observed days, and every attributed value names its method and its evidence.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::tags::{Cause, Family, Label, Tags};

/// The longest gap, in calendar days, a transition bridges back to the unit's last attributed day
/// (Decision 18).
pub const INHERIT_DAYS: i64 = 14;

/// What one reason says, tag by tag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed<'a> {
    /// Each cause with the tag that named it, in reason order.
    pub causes: Vec<(&'a Cause, String)>,
    pub flags: Vec<&'a str>,
    /// Whether a tag said the unit is increasing, holding or starting up.
    pub transition: bool,
    /// EN references, with their numbers where the tag carried one.
    pub xrefs: Vec<Option<u32>>,
    /// Tags accepted on purpose as Unattributed.
    pub unclassified: Vec<String>,
    /// Tags nothing knows. The backfill fails on these, and the daily ingest quarantines them as
    /// `unknown_tag` (Decision 21).
    pub unknown: Vec<String>,
}

/// Observes `reason` (a status page's Reason or Comment cell) through `tags`.
#[must_use]
pub fn observe<'a>(tags: &'a Tags, reason: &str) -> Observed<'a> {
    let mut observed = Observed::default();
    for tag in crate::tags::tags(reason) {
        match tags.classify(&tag).map(|classified| classified.label) {
            Some(Label::Cause(cause)) => observed.causes.push((cause, tag)),
            Some(Label::Flag(flag)) => {
                if !observed.flags.contains(&flag) {
                    observed.flags.push(flag);
                }
            }
            Some(Label::Transition) => observed.transition = true,
            Some(Label::Xref(number)) => observed.xrefs.push(number),
            Some(Label::Unclassified) => observed.unclassified.push(tag),
            None => observed.unknown.push(tag),
        }
    }
    observed
}

/// The inference that produced an attributed cause, or why there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    /// The highest-precedence cause on the day (Decision 19).
    #[serde(rename = "precedence_v1")]
    PrecedenceV1,
    /// A transition that continued the unit's last attributed cause, at most [`INHERIT_DAYS`] back.
    /// Inherited days extend the chain, so a run of transitions carries one cause through.
    #[serde(rename = "inherit_14d")]
    Inherit14d,
    /// Only EN references, not yet resolved through the Event Notifications (PRD-0003).
    #[serde(rename = "xref_unresolved")]
    XrefUnresolved,
    /// Nothing on the day names a cause.
    #[serde(rename = "no_cause")]
    NoCause,
    /// The page is inside its 28-day hold, so its reason text isn't published yet (Decision 12).
    #[serde(rename = "reason_pending")]
    ReasonPending,
    /// The NRC published no report for the day, and the power comes from EIA (Decision 11).
    #[serde(rename = "no_report")]
    NoReport,
    /// The reason carries a tag nothing knows, so the row is quarantined (Decision 21).
    #[serde(rename = "unknown_tag")]
    UnknownTag,
}

impl Method {
    /// The name stored with the label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrecedenceV1 => "precedence_v1",
            Self::Inherit14d => "inherit_14d",
            Self::XrefUnresolved => "xref_unresolved",
            Self::NoCause => "no_cause",
            Self::ReasonPending => "reason_pending",
            Self::NoReport => "no_report",
            Self::UnknownTag => "unknown_tag",
        }
    }
}

/// What an attributed cause rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// The tag that named the cause.
    Tag(String),
    /// The day whose observed cause a transition took, which starts the chain the transition is on.
    InheritedFrom(NaiveDate),
    /// The EN references, numbered where the tag carried one.
    Xrefs(Vec<Option<u32>>),
    /// The tags nothing knows, on a quarantined row.
    Unknown(Vec<String>),
    None,
}

/// One unit-day's attributed cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attributed<'a> {
    /// `None` is Unattributed.
    pub cause: Option<&'a Cause>,
    pub method: Method,
    pub evidence: Evidence,
}

impl Attributed<'_> {
    /// An Unattributed value, with the method that says why.
    #[must_use]
    pub const fn unattributed(method: Method, evidence: Evidence) -> Self {
        Self { cause: None, method, evidence }
    }

    /// The family the barcode colors this unit-day with.
    #[must_use]
    pub fn family(&self) -> Family {
        self.cause.map_or(Family::Unattributed, |cause| cause.family)
    }
}

/// Decision 19's order, highest first: Forced > Refueling > Maintenance > Equipment >
/// Environmental > Grid > Fuel cycle > Unattributed.
const fn precedence_v1(family: Family) -> u8 {
    match family {
        Family::Forced => 0,
        Family::Refueling => 1,
        Family::Maintenance => 2,
        Family::Equipment => 3,
        Family::Environmental => 4,
        Family::Grid => 5,
        Family::FuelCycle => 6,
        Family::Unattributed => 7,
    }
}

/// Attributes one unit's days, one at a time and in date order, so a rebuild can stream them.
/// Days never passed in still count toward the inheritance gap, since it is measured in calendar
/// days.
#[derive(Debug, Clone, Default)]
pub struct Attributor<'a> {
    /// The last attributed day, the day its cause was observed, and the cause.
    last: Option<(NaiveDate, NaiveDate, &'a Cause)>,
}

impl<'a> Attributor<'a> {
    /// Attributes `date`, which must follow every day passed in before it.
    pub fn attribute(&mut self, date: NaiveDate, observed: &Observed<'a>) -> Attributed<'a> {
        // Ties keep the cause named first, since `min_by_key` returns the first minimum.
        if let Some((cause, tag)) = observed.causes.iter().min_by_key(|(cause, _)| precedence_v1(cause.family)) {
            self.last = Some((date, date, cause));
            return Attributed {
                cause: Some(cause),
                method: Method::PrecedenceV1,
                evidence: Evidence::Tag(tag.clone()),
            };
        }
        if observed.transition {
            return match self.last.filter(|(previous, ..)| (date - *previous).num_days() <= INHERIT_DAYS) {
                Some((_, origin, cause)) => {
                    self.last = Some((date, origin, cause));
                    Attributed {
                        cause: Some(cause),
                        method: Method::Inherit14d,
                        evidence: Evidence::InheritedFrom(origin),
                    }
                }
                None => Attributed::unattributed(Method::Inherit14d, Evidence::None),
            };
        }
        if !observed.xrefs.is_empty() {
            return Attributed::unattributed(Method::XrefUnresolved, Evidence::Xrefs(observed.xrefs.clone()));
        }
        Attributed::unattributed(Method::NoCause, Evidence::None)
    }
}

/// Attributes one unit's days, which must be in date order (see [`Attributor`]).
#[must_use]
pub fn attribute<'a>(days: &[(NaiveDate, Observed<'a>)]) -> Vec<Attributed<'a>> {
    let mut attributor = Attributor::default();
    days.iter().map(|(date, observed)| attributor.attribute(*date, observed)).collect()
}
