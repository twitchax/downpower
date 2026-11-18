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

/// The scram check (PRD-0003 T-007) found the scram rule catching control rod scram time tests,
/// planned derates no Event Notification reports.
#[test]
fn a_scram_time_test_is_testing_not_a_scram() {
    let tags = Tags::catalog().unwrap();
    let tests = [
        "holding power for scram time testing",
        "control rod scram time testing",
        "testing control rod scram times",
        "power ascension on hold for scram timing",
        "reduced power for turbine test, scram test and rod sequence exchange",
        "single control rod scram during rps testing",
        "downpower manuever to 60% to perform single rod scrams and planned 345 kv line outage on line #381",
    ];
    for tag in tests {
        assert_eq!(label(&tags, tag), "testing/Maintenance", "{tag}");
    }
    assert_eq!(label(&tags, "automatic reactor scram. see event # 41243"), "scram/Forced");
}

/// The scram check (PRD-0003 T-007) found the scram rule's bare `tripped` catching pumps, lines and
/// breakers. A reactor trip names the reactor, or is a manual trip, or opens the tag.
#[test]
fn a_tripped_pump_line_or_breaker_is_not_a_reactor_scram() {
    let tags = Tags::catalog().unwrap();
    let cases = [
        ("one recirc pump tripped", "recirculation_pump/Equipment"),
        ("tripped 1 of 5 recirc pumps", "recirculation_pump/Equipment"),
        ("reduced power due to a tripped reactor feed pump", "feedwater/Equipment"),
        ("rfp tripped. increasing to 90% power to hold and troubleshoot.", "feedwater/Equipment"),
        ("reducing power to 15 percent rtp where turbine will be tripped for maintenance on turbine.", "maintenance/Maintenance"),
        ("reactor manually tripped from 100% power - see event # 39720", "scram/Forced"),
        ("manually tripped due to high rcp motor bearing temp - see en#48475", "scram/Forced"),
        ("tripped. see en #44318.", "scram/Forced"),
        ("tripped reactor for startup testing", "scram/Forced"),
    ];
    for (tag, expected) in cases {
        assert_eq!(label(&tags, tag), expected, "{tag}");
    }
}

/// The scram check (PRD-0003 T-007) found the scram rule catching tests of the turbine and reactor
/// trip systems. A reactor that scrams during one is still a scram.
#[test]
fn a_trip_system_test_is_testing_not_a_scram() {
    let tags = Tags::catalog().unwrap();
    let tests = [
        "turbine trip testing in progress",
        "turbine overspeed trip testing",
        "holding power for scram rod testing",
        "reactor trip testing",
        "prepare for planned plant trip test",
    ];
    for tag in tests {
        assert_eq!(label(&tags, tag), "testing/Maintenance", "{tag}");
    }
    let scrams = [
        "rx scram during turbine trip testing. see en#45265",
        "reactor scrammed during the oil trip test portion of the turbine weekly surveillance test. see event # 36501.",
        "reactor trip during turbine trip system testing - see event #36148",
    ];
    for tag in scrams {
        assert_eq!(label(&tags, tag), "scram/Forced", "{tag}");
    }
}

/// The scram check (PRD-0003 T-007) found the scram rule catching work on scram valves, the scram
/// discharge volume and turbine trip solenoids, which are parts of a trip system, not trips.
#[test]
fn work_on_trip_system_parts_is_maintenance_not_a_scram() {
    let tags = Tags::catalog().unwrap();
    let parts = [
        "isolating leaking scram outlet valve",
        "emergent load drop to repair scram discharge volume valves",
        "reduced power for planned crd scram solenoid pilot valve repair",
        "holding for turbine trip system maintenance",
        "holding power at this level with the unit off line as part of a maintenance outage to repair a turbine trip solenoid",
    ];
    for tag in parts {
        assert_eq!(label(&tags, tag), "maintenance/Maintenance", "{tag}");
    }
    assert_eq!(label(&tags, "see event #35753, reactor scram due to high level in the scram discharge volume"), "scram/Forced");
}

/// The scram check (PRD-0003 T-007) found turbine and generator trips on units no Event Notification
/// reports a reactor scram on. A trip that names no reactor trip takes the cause the tag names, else
/// `turbine_generator`; a tag whose unit's event reports a reactor scram is a key.
#[test]
fn a_turbine_or_generator_trip_is_not_a_reactor_scram() {
    let tags = Tags::catalog().unwrap();
    let cases = [
        ("turbine trip", "turbine_generator/Equipment"),
        ("investigating turbine trip", "turbine_generator/Equipment"),
        ("turbine tripped due to lightning strike", "turbine_generator/Equipment"),
        ("turbine trip due to ehc rupture", "turbine_generator/Equipment"),
        ("main generator trip shortly after syncing to grid. investigation in progress", "turbine_generator/Equipment"),
        ("turbine trip due to transformer issue. see en#46025.", "transformer/Equipment"),
        ("holding power for troubleshooting feedwater turbine trip", "feedwater/Equipment"),
        ("3b recirc motor generator tripped", "recirculation_pump/Equipment"),
        ("maintenance outage - turbine trip", "maintenance/Maintenance"),
    ];
    for (tag, expected) in cases {
        assert_eq!(label(&tags, tag), expected, "{tag}");
    }
    let scrams = [
        "automatic reactor shutdown due to main turbine trip - see en #45789.",
        "scram due to turbine trip from high vibration - see event #42180",
        "starting up after turbine trip and scram - increasing power. see event #41832",
        "turbine trip. see event 42966",
        "en56478 - control system power failure and turbine trip",
        "investigate cause of main generator trip",
    ];
    for tag in scrams {
        assert_eq!(label(&tags, tag), "scram/Forced", "{tag}");
    }
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
