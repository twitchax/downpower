//! The two label layers (DESIGN.md Decisions 18–20). [`observe`] turns a reason into what its tags
//! say, which is checkable against the text. [`attribute`] infers one primary cause per unit-day
//! from a unit's observed days, and every attributed value names its method and its evidence.

use chrono::NaiveDate;

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

/// The inference that produced an attributed cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The highest-precedence cause on the day (Decision 19).
    PrecedenceV1,
    /// A transition that continued the unit's last attributed cause, at most [`INHERIT_DAYS`] back.
    /// Inherited days extend the chain, so a run of transitions carries one cause through.
    Inherit14d,
    /// Only EN references, not yet resolved through the Event Notifications (PRD-0003).
    XrefUnresolved,
    /// Nothing on the day names a cause.
    NoCause,
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
        }
    }
}

/// What an attributed cause rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// The tag that named the cause.
    Tag(String),
    /// The day whose observed cause a transition took, which starts the chain the transition is on.
    InheritedFrom(NaiveDate),
    /// The EN references, numbered where the tag carried one.
    Xrefs(Vec<Option<u32>>),
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

/// Attributes one unit's days, which must be in date order. Days missing from the slice still
/// count toward the inheritance gap, since it is measured in calendar days.
#[must_use]
pub fn attribute<'a>(days: &[(NaiveDate, Observed<'a>)]) -> Vec<Attributed<'a>> {
    // The last attributed day, the day its cause was observed, and the cause.
    let mut last: Option<(NaiveDate, NaiveDate, &'a Cause)> = None;
    days.iter()
        .map(|(date, observed)| {
            // Ties keep the cause named first, since `min_by_key` returns the first minimum.
            if let Some((cause, tag)) = observed.causes.iter().min_by_key(|(cause, _)| precedence_v1(cause.family)) {
                last = Some((*date, *date, cause));
                return Attributed {
                    cause: Some(cause),
                    method: Method::PrecedenceV1,
                    evidence: Evidence::Tag(tag.clone()),
                };
            }
            if observed.transition {
                return match last.filter(|(previous, ..)| (*date - *previous).num_days() <= INHERIT_DAYS) {
                    Some((_, origin, cause)) => {
                        last = Some((*date, origin, cause));
                        Attributed {
                            cause: Some(cause),
                            method: Method::Inherit14d,
                            evidence: Evidence::InheritedFrom(origin),
                        }
                    }
                    None => Attributed {
                        cause: None,
                        method: Method::Inherit14d,
                        evidence: Evidence::None,
                    },
                };
            }
            if !observed.xrefs.is_empty() {
                return Attributed {
                    cause: None,
                    method: Method::XrefUnresolved,
                    evidence: Evidence::Xrefs(observed.xrefs.clone()),
                };
            }
            Attributed {
                cause: None,
                method: Method::NoCause,
                evidence: Evidence::None,
            }
        })
        .collect()
}
