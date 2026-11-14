//! The committed tag table against the archive's reason snapshot (PRD-0002 uat-002). The snapshot,
//! `catalog/reasons.tsv`, is every distinct tag on the status pages (`cargo xtask reasons`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;

use pretty_assertions::assert_eq;

use downpower_ingest::tags::{Family, Label, Matched, Tags};

const REASONS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog/reasons.tsv"));

fn snapshot() -> impl Iterator<Item = &'static str> {
    REASONS.lines().skip(1).map(|line| line.split('\t').next().unwrap())
}

#[test]
fn uat_reasons_snapshot_every_tag_is_known() {
    let tags = Tags::catalog().unwrap();

    let unknown: Vec<_> = snapshot().filter(|tag| tags.classify(tag).is_none()).collect();

    assert_eq!(unknown, Vec::<&str>::new(), "add a key or rule, or run `cargo xtask unclassified` and review the diff");
    assert_eq!(snapshot().count(), 21_864);
}

#[test]
fn uat_reasons_snapshot_the_accepted_list_holds_only_tags_nothing_else_knows() {
    let tags = Tags::catalog().unwrap();
    let seen: HashSet<_> = snapshot().collect();

    let mut stale: Vec<_> = tags.unclassified().filter(|tag| tags.matches(tag) || !seen.contains(tag)).collect();
    stale.sort_unstable();

    assert_eq!(stale, Vec::<&str>::new(), "run `cargo xtask unclassified` and review the diff");
}

/// The fine cause and family a tag classifies to, or what else it is.
fn label(tags: &Tags, tag: &str) -> String {
    match tags.classify(tag).unwrap().label {
        Label::Cause(cause) => format!("{}/{:?}", cause.name, cause.family),
        other => format!("{other:?}"),
    }
}

#[test]
fn uat_reasons_snapshot_the_design_examples_classify_as_designed() {
    let tags = Tags::catalog().unwrap();
    let cases = [
        ("refueling outage", "refueling_outage/Refueling"),
        ("coastdown to refueling outage", "coastdown/FuelCycle"),
        ("reduced power for rod pattern adjustment", "rod_pattern/FuelCycle"),
        ("forced outage - see en #44507", "forced_outage/Forced"),
        ("reactor trip due to main transformer fire. see en# 52210.", "scram/Forced"),
        ("reduced power for planned maintenance", "maintenance/Maintenance"),
        ("transformer limitation", "transformer/Equipment"),
        ("100% electrical capability", "full_electrical_output/Equipment"),
        ("100% of rated electrical capability", "full_electrical_output/Equipment"),
        ("limited by turbine controls", "rated_output_limit/Equipment"),
        ("steam generator limitations", "steam_generator/Equipment"),
        ("main condenser inefficiencies", "main_condenser_inefficiencies/Equipment"),
        ("single loop ops due to recirculation pump trip", "recirculation_pump/Equipment"),
        ("reduced power due to environmental conditions", "environmental_unspecified/Environmental"),
        ("limited by cooling water discharge temperatures", "heat/Environmental"),
        ("reduced power due to grass influx at intake", "marine_life/Environmental"),
        ("limited to less than 1250 mwe. 300 mvar minimum.", "grid_limit/Grid"),
        ("reduced power for economic dispatch", "economic_dispatch/Grid"),
        ("hot weather alert", "Flag(\"hot_weather_alert\")"),
        ("maximum generation alert", "Flag(\"max_generation_alert\")"),
        ("yellow grid status", "Flag(\"grid_condition\")"),
        ("hands off grid restriction", "Flag(\"grid_condition\")"),
        ("conservative operations", "Flag(\"conservative_ops\")"),
        ("contingency for limit to 1100 mwe net", "Flag(\"grid_contingency\")"),
        ("increasing power", "Transition"),
        ("startup in progress", "Transition"),
        ("defueled", "Transition"),
        ("see en# 55049", "Xref(Some(55049))"),
    ];
    for (tag, expected) in cases {
        assert_eq!(label(&tags, tag), expected, "{tag}");
    }
}

#[test]
fn uat_reasons_snapshot_keys_win_over_rules() {
    let tags = Tags::catalog().unwrap();

    let classified = tags.classify("scram time testing").unwrap();

    assert_eq!(classified.matched, Matched::Key);
    assert!(matches!(classified.label, Label::Cause(cause) if cause.family == Family::Maintenance));
}

#[test]
fn uat_reasons_snapshot_only_full_electrical_output_books_no_lost_output() {
    let tags = Tags::catalog().unwrap();

    let label = |tag: &str| tags.classify(tag).unwrap().label;
    let Label::Cause(full) = label("100% electrical capability") else { panic!("not a cause") };
    let Label::Cause(limited) = label("limited by turbine controls") else { panic!("not a cause") };

    assert!(!full.lost_output);
    assert!(limited.lost_output);
}
