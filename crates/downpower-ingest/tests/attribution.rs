//! The observed and attributed layers over the committed tag table (PRD-0002 uat-003).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Days, NaiveDate};
use pretty_assertions::assert_eq;

use downpower_ingest::attribution::{self, Attributed, Evidence, Method, Observed};
use downpower_ingest::tags::{Family, Tags};

/// Day `n` counted from July 1, 2025 (day 1), so day 32 is August 1.
fn day(n: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 7, 1).unwrap() + Days::new(u64::from(n) - 1)
}

/// One unit's days, each `(day, reason)`.
fn attribute<'a>(tags: &'a Tags, days: &[(u32, &str)]) -> Vec<Attributed<'a>> {
    let observed: Vec<(NaiveDate, Observed<'a>)> = days.iter().map(|&(n, reason)| (day(n), attribution::observe(tags, reason))).collect();
    attribution::attribute(&observed)
}

/// `(cause name or "-", method, evidence)` for compact comparisons.
fn summary(attributed: &Attributed<'_>) -> (String, &'static str, Evidence) {
    (
        attributed.cause.map_or_else(|| "-".to_owned(), |cause| cause.name.clone()),
        attributed.method.as_str(),
        attributed.evidence.clone(),
    )
}

fn tag(text: &str) -> Evidence {
    Evidence::Tag(text.to_owned())
}

#[test]
fn uat_attribution_precedence_v1_picks_the_highest_family_and_names_its_tag() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(
        &tags,
        &[
            (1, "Reduced Power for Maintenance; Main Condenser Inefficiencies"),
            (2, "Coastdown to Refueling Outage;\r\n\r\nTransformer Limitation"),
            (3, "Steam Generator Limitations;\n\nMaximum Generation Alert"),
            (4, "Forced Outage; See EN #44507"),
        ],
    );

    assert_eq!(
        attributed.iter().map(summary).collect::<Vec<_>>(),
        [
            ("maintenance".to_owned(), "precedence_v1", tag("reduced power for maintenance")),
            ("transformer".to_owned(), "precedence_v1", tag("transformer limitation")),
            ("steam_generator".to_owned(), "precedence_v1", tag("steam generator limitations")),
            ("forced_outage".to_owned(), "precedence_v1", tag("forced outage")),
        ]
    );
}

#[test]
fn uat_attribution_inherit_14d_bridges_at_most_14_days_back_to_an_attributed_day() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(&tags, &[(1, "Refueling Outage"), (3, "Startup in Progress"), (17, "Increasing Power"), (32, "Holding Power")]);

    assert_eq!(
        attributed.iter().map(summary).collect::<Vec<_>>(),
        [
            ("refueling_outage".to_owned(), "precedence_v1", tag("refueling outage")),
            ("refueling_outage".to_owned(), "inherit_14d", Evidence::InheritedFrom(day(1))),
            ("refueling_outage".to_owned(), "inherit_14d", Evidence::InheritedFrom(day(1))),
            ("-".to_owned(), "inherit_14d", Evidence::None),
        ]
    );
    assert_eq!(attributed[3].family(), Family::Unattributed);
}

#[test]
fn uat_attribution_inherit_14d_chains_through_a_run_of_transitions() {
    let tags = Tags::catalog().unwrap();
    let mut days = vec![(1, "REFUELING OUTAGE")];
    days.extend((2..=31).map(|n| (n, "DEFUELED")));

    let attributed = attribute(&tags, &days);

    for value in &attributed[1..] {
        assert_eq!(summary(value), ("refueling_outage".to_owned(), "inherit_14d", Evidence::InheritedFrom(day(1))));
    }
}

#[test]
fn uat_attribution_a_newly_observed_cause_restarts_the_chain() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(&tags, &[(1, "Refueling Outage"), (2, "Increasing Power"), (3, "Forced Outage"), (4, "Startup in Progress")]);

    assert_eq!(summary(&attributed[3]), ("forced_outage".to_owned(), "inherit_14d", Evidence::InheritedFrom(day(3))));
}

#[test]
fn uat_attribution_days_without_a_cause_neither_extend_nor_break_the_chain() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(&tags, &[(1, "Refueling Outage"), (8, ""), (15, "Increasing Power"), (16, ""), (30, "Holding Power")]);

    assert_eq!(summary(&attributed[2]), ("refueling_outage".to_owned(), "inherit_14d", Evidence::InheritedFrom(day(1))));
    assert_eq!(summary(&attributed[4]), ("-".to_owned(), "inherit_14d", Evidence::None));
}

#[test]
fn uat_attribution_an_unresolved_xref_is_unattributed_and_keeps_its_number() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(&tags, &[(1, "SEE EN #49154")]);

    assert_eq!(summary(&attributed[0]), ("-".to_owned(), "xref_unresolved", Evidence::Xrefs(vec![Some(49154)])));
    assert_eq!(attributed[0].family(), Family::Unattributed);
}

#[test]
fn uat_attribution_environmental_conditions_stay_unspecified() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(&tags, &[(1, "Reduced Power Due to Environmental Conditions")]);

    assert_eq!(attributed[0].cause.unwrap().name, "environmental_unspecified");
    assert_eq!(attributed[0].family(), Family::Environmental);
}

#[test]
fn uat_attribution_flags_alone_name_no_cause() {
    let tags = Tags::catalog().unwrap();

    let observed = attribution::observe(&tags, "Hot Weather Alert; Conservative Operations; Hot Weather Alert");
    let attributed = attribution::attribute(&[(day(1), observed.clone())]);

    assert_eq!(observed.flags, ["hot_weather_alert", "conservative_ops"]);
    assert_eq!(summary(&attributed[0]), ("-".to_owned(), "no_cause", Evidence::None));
}

#[test]
fn uat_attribution_observe_reports_unknown_and_accepted_tags() {
    let tags = Tags::catalog().unwrap();

    let observed = attribution::observe(&tags, ".; Zorbulating the flux capacitor; Refueling Outage");

    assert_eq!(observed.unclassified, ["."]);
    assert_eq!(observed.unknown, ["zorbulating the flux capacitor"]);
    assert_eq!(observed.causes.len(), 1);
}

#[test]
fn uat_attribution_every_value_names_a_method_that_matches_its_evidence() {
    let tags = Tags::catalog().unwrap();

    let attributed = attribute(&tags, &[(1, "Forced Outage"), (2, "Increasing Power"), (3, "See EN #1"), (4, ""), (30, "Holding Power")]);

    for value in &attributed {
        let consistent = match value.method {
            Method::PrecedenceV1 => value.cause.is_some() && matches!(value.evidence, Evidence::Tag(_)),
            Method::Inherit14d => value.cause.is_some() == matches!(value.evidence, Evidence::InheritedFrom(_)),
            Method::XrefUnresolved => value.cause.is_none() && matches!(value.evidence, Evidence::Xrefs(_)),
            Method::NoCause => value.cause.is_none() && value.evidence == Evidence::None,
            Method::ReasonPending | Method::NoReport | Method::UnknownTag => false,
        };
        assert!(consistent, "{value:?}");
    }
}

#[test]
fn uat_attribution_methods_serialize_as_their_stored_names() {
    let methods = [
        Method::PrecedenceV1,
        Method::Inherit14d,
        Method::XrefUnresolved,
        Method::NoCause,
        Method::ReasonPending,
        Method::NoReport,
        Method::UnknownTag,
    ];
    for method in methods {
        assert_eq!(serde_json::to_value(method).unwrap(), method.as_str());
        assert_eq!(serde_json::from_value::<Method>(method.as_str().into()).unwrap(), method);
    }
}

#[test]
fn uat_attribution_evidence_serializes_by_kind() {
    assert_eq!(
        serde_json::to_value(Evidence::Tag("refueling outage".to_owned())).unwrap(),
        serde_json::json!({"tag": "refueling outage"})
    );
    assert_eq!(serde_json::to_value(Evidence::InheritedFrom(day(1))).unwrap(), serde_json::json!({"inherited_from": "2025-07-01"}));
    assert_eq!(serde_json::to_value(Evidence::Xrefs(vec![Some(49154), None])).unwrap(), serde_json::json!({"xrefs": [49154, null]}));
    assert_eq!(serde_json::to_value(Evidence::None).unwrap(), serde_json::json!("none"));
}
