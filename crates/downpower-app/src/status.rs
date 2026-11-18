//! `/status`: what the ledger rests on (PRD-0002 T-015, PRD-0003 T-010). The current build's
//! coverage by source, the NRC/EIA disagreement rate, both scram checks, what the classifier
//! quarantined, the Event Notification archive, and the update loop's recent runs. The types mirror the subset of `/api/status` shown here; the server's tests parse its
//! output into them, so the two can't drift apart.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use downpower_core::scrams::Counts;
use leptos::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Status {
    pub build: Option<Build>,
    /// The table the views read; set while `build` is null only when a rebuild is replacing a
    /// build record an older binary wrote.
    #[serde(default)]
    pub serving: Option<String>,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Build {
    pub version: String,
    pub built_at: String,
    pub summary: Summary,
    /// Builds from before the event set (PRD-0003 T-003) don't have it.
    #[serde(default)]
    pub events: Option<Archive>,
}

/// What a build found in the stored Event Notification pages.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Archive {
    pub pages: u64,
    /// Pages reading "No events found".
    pub empty: u64,
    pub events: u64,
    /// Events by category label; builds from before PRD-0003 T-010 don't have it.
    #[serde(default)]
    pub categories: BTreeMap<String, u64>,
    /// Dates whose page failed to parse.
    pub failed: Vec<NaiveDate>,
    /// Facility names the catalogs don't know, with their events.
    pub unknown_facilities: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Summary {
    pub days: u64,
    pub first: Option<NaiveDate>,
    pub last: Option<NaiveDate>,
    pub compared: u64,
    pub disagreed: u64,
    pub unknown_tags: BTreeMap<String, u64>,
    pub unknown_units: BTreeMap<String, u64>,
    pub lost_kwh: u64,
    pub without_capacity: u64,
    pub dates: Coverage,
    /// Builds from before the scram check (PRD-0003 T-007) don't have it.
    #[serde(default)]
    pub scrams: Counts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Coverage {
    pub reasons: u64,
    pub held: u64,
    pub eia: u64,
    pub missing: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Run {
    pub started_at: String,
    pub finished_at: String,
    pub status: Option<Fetched>,
    pub events: Option<Fetched>,
    pub nuc_status: Option<NaiveDate>,
    /// Runs before PRD-0003 T-008 don't have it.
    #[serde(default)]
    pub scram_dataset: Option<NaiveDate>,
    pub rebuilt: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Fetched {
    pub stored: u64,
    /// Refetched and found unchanged; runs before 2026-11-15 don't have it.
    #[serde(default)]
    pub unchanged: u64,
    pub missing: u64,
    pub skipped: u64,
}

/// An RFC 3339 timestamp as `2026-11-14 20:22 UTC`.
fn when(timestamp: &str) -> String {
    match (timestamp.get(..10), timestamp.get(11..16)) {
        (Some(date), Some(time)) => format!("{date} {time} UTC"),
        _ => timestamp.to_owned(),
    }
}

/// A count with thousands separators: `1,016,840`.
fn thousands(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `part` of `whole` as a percentage with `digits` decimals.
fn share(part: u64, whole: u64, digits: usize) -> String {
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let percent = if whole == 0 { 0.0 } else { part as f64 / whole as f64 * 100.0 };
    format!("{percent:.digits$}%")
}

/// The scram check in words, or `None` for a build from before it.
fn scrams(counts: Counts) -> Option<String> {
    let (events, reports) = (counts.events, counts.reports);
    (events.checked() + reports.checked() > 0).then(|| {
        format!(
            "The NRC's two sources on scrams: the status reports show {} of {} scrams an event report lists ({}), and an event report lists {} of {} scrams the status reports mark ({}).",
            thousands(events.agreed()),
            thousands(events.checked()),
            share(events.agreed(), events.checked(), 1),
            thousands(reports.agreed()),
            thousands(reports.checked()),
            share(reports.agreed(), reports.checked(), 1)
        )
    })
}

/// The dataset check in words, or `None` for a build without a stored dataset.
fn dataset(counts: Counts) -> Option<String> {
    counts.dataset.map(|dataset| {
        format!(
            "Against the NRC's scram dataset, {} to {}, the event reports and the dataset both list {} of {} scrams ({}), leaving aside {} known differences: retractions, and scrams from a reactor not critical that the dataset leaves out.",
            dataset.first,
            dataset.last,
            thousands(dataset.agreed()),
            thousands(dataset.checked()),
            share(dataset.agreed(), dataset.checked(), 1),
            thousands(dataset.known)
        )
    })
}

/// How many failed dates the archive sentence names before it stops.
const FAILED_SHOWN: usize = 10;

/// The event archive in words: its pages and events, and the pages that failed to parse.
fn events(archive: &Archive) -> String {
    let held = format!(
        "{} Event Notification pages, {} of them reading \"No events found\", hold {} events.",
        thousands(archive.pages),
        thousands(archive.empty),
        thousands(archive.events)
    );
    let failed = match archive.failed.as_slice() {
        [] => "Every page parsed.".to_owned(),
        dates => {
            let mut named: Vec<String> = dates.iter().take(FAILED_SHOWN).map(NaiveDate::to_string).collect();
            if dates.len() > FAILED_SHOWN {
                named.push(format!("and {} more", dates.len() - FAILED_SHOWN));
            }
            let (pages, were) = if dates.len() == 1 { ("page", "was") } else { ("pages", "were") };
            format!("{} {pages} failed to parse and {were} skipped: {}.", dates.len(), named.join(", "))
        }
    };
    format!("{held} {failed}")
}

/// The archive's categories, most events first.
fn by_category(archive: &Archive) -> Vec<(String, u64)> {
    let mut categories: Vec<(String, u64)> = archive.categories.iter().map(|(label, events)| (label.clone(), *events)).collect();
    categories.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    categories
}

/// A fetch summary in words.
fn fetched(label: &str, fetched: Option<Fetched>) -> String {
    fetched.map_or_else(
        || format!("{label}: not fetched"),
        |fetched| {
            format!(
                "{label}: {} stored, {} unchanged, {} missing, {} settled",
                fetched.stored, fetched.unchanged, fetched.missing, fetched.skipped
            )
        },
    )
}

#[cfg(feature = "hydrate")]
async fn load() -> Result<Status, String> {
    let bytes = crate::browser::fetch("/api/status").await?.ok_or("no status")?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load() -> Result<Status, String> {
    Err("the status loads in the browser".to_owned())
}

#[component]
pub fn StatusPage() -> impl IntoView {
    let data = LocalResource::new(load);
    view! {
        <header class="intro">
            <h1>"Status"</h1>
            <p class="muted">"What the ledger rests on, and what the update loop has done lately."</p>
        </header>
        {move || match data.get() {
            None => view! { <p class="meta">"loading…"</p> }.into_any(),
            Some(Err(error)) => view! { <p class="meta">{error}</p> }.into_any(),
            Some(Ok(status)) => view! {
                <Events archive=status.build.as_ref().and_then(|build| build.events.clone()) />
                <Ledger current=status.build serving=status.serving />
                <Runs runs=status.runs />
            }
            .into_any(),
        }}
    }
}

/// What the ledger panel says without a build it can read: a deploy that changed the build record
/// still serves the previous build while the new one runs.
fn pending(serving: bool) -> &'static str {
    if serving {
        "Rebuilding after a deploy: the site serves the previous build until the new one is done."
    } else {
        "No build yet: the first one is running."
    }
}

/// The current build: coverage, agreement and quarantine.
#[component]
fn Ledger(current: Option<Build>, serving: Option<String>) -> impl IntoView {
    let Some(build) = current else {
        return view! { <section class="panel"><p class="muted">{pending(serving.is_some())}</p></section> }.into_any();
    };
    let summary = build.summary;
    let dates = summary.dates;
    let total = dates.reasons + dates.held + dates.eia + dates.missing;
    let coverage = [
        ("NRC page, with reasons", dates.reasons),
        ("NRC page inside the 28-day hold: numbers only", dates.held),
        ("No NRC page: EIA's numbers only", dates.eia),
        ("Missing: neither source", dates.missing),
    ]
    .map(|(label, count)| view! { <tr><th scope="row">{label}</th><td>{thousands(count)}</td><td>{share(count, total, 2)}</td></tr> })
    .collect_view();
    let quarantined = summary
        .unknown_tags
        .iter()
        .map(|(tag, uses)| view! { <tr><th scope="row">{tag.clone()}</th><td>{thousands(*uses)}</td></tr> })
        .chain(
            summary
                .unknown_units
                .iter()
                .map(|(unit, uses)| view! { <tr><th scope="row">{format!("unit: {unit}")}</th><td>{thousands(*uses)}</td></tr> }),
        )
        .collect_view();
    let span = format!(
        "{} to {}",
        summary.first.map(|date| date.to_string()).unwrap_or_default(),
        summary.last.map(|date| date.to_string()).unwrap_or_default()
    );
    // kWh to the nearest TWh.
    let twh = summary.lost_kwh.saturating_add(500_000_000) / 1_000_000_000;
    view! {
        <section class="panel">
            <div class="label">"Ledger"</div>
            <p>
                {format!("{} unit-days, {span}, built {} (derived version {}). ", thousands(summary.days), when(&build.built_at), build.version)}
                {format!("{} TWh of reported power reduction; {} reduced unit-days have no capacity to weigh them.", thousands(twh), thousands(summary.without_capacity))}
            </p>
            <table class="totals">
                <caption class="meta">"Dates by source"</caption>
                <tbody>{coverage}</tbody>
            </table>
            <p>
                {format!(
                    "NRC and EIA disagree on power on {} of {} unit-days both report ({}).",
                    thousands(summary.disagreed),
                    thousands(summary.compared),
                    share(summary.disagreed, summary.compared, 3)
                )}
            </p>
            {scrams(summary.scrams).map(|sentence| view! { <p>{sentence}</p> })}
            {dataset(summary.scrams).map(|sentence| view! { <p>{sentence}</p> })}
            {if summary.unknown_tags.is_empty() && summary.unknown_units.is_empty() {
                view! { <p class="meta">"Nothing quarantined: every reason tag and unit name is known."</p> }.into_any()
            } else {
                view! {
                    <table class="totals">
                        <caption class="meta">"Quarantined, counted as Unattributed until the catalog learns them"</caption>
                        <tbody>{quarantined}</tbody>
                    </table>
                }
                .into_any()
            }}
        </section>
    }
    .into_any()
}

/// The Event Notification archive: its pages and events, by category, and the facility names
/// quarantined until the catalogs learn them.
#[component]
fn Events(archive: Option<Archive>) -> impl IntoView {
    archive.map(|archive| {
        let categories = by_category(&archive)
            .into_iter()
            .map(|(label, count)| view! { <tr><th scope="row">{label}</th><td>{thousands(count)}</td><td>{share(count, archive.events, 1)}</td></tr> })
            .collect_view();
        let facilities = archive
            .unknown_facilities
            .iter()
            .map(|(name, count)| view! { <tr><th scope="row">{name.clone()}</th><td>{thousands(*count)}</td></tr> })
            .collect_view();
        view! {
            <section class="panel">
                <div class="label">"Event Notifications"</div>
                <p>{events(&archive)}</p>
                <table class="totals">
                    <caption class="meta">"Events by category"</caption>
                    <tbody>{categories}</tbody>
                </table>
                {if archive.unknown_facilities.is_empty() {
                    view! { <p class="meta">"Nothing quarantined: every facility name is known."</p> }.into_any()
                } else {
                    view! {
                        <table class="totals">
                            <caption class="meta">"Facility names quarantined until the catalogs learn them, with their events"</caption>
                            <tbody>{facilities}</tbody>
                        </table>
                    }
                    .into_any()
                }}
            </section>
        }
    })
}

/// The update loop's newest runs, each a marker and a sentence.
#[component]
fn Runs(runs: Vec<Run>) -> impl IntoView {
    let items = runs
        .into_iter()
        .map(|run| {
            let failed = !run.errors.is_empty();
            let errors = run.errors.join("; ");
            view! {
                <li class:failed=failed>
                    <span class=if failed { "marker fault" } else { "marker pass" } aria-hidden="true"></span>
                    <span class="state">{if failed { "failed" } else { "passed" }}</span>
                    <span class="meta">{when(&run.started_at)}</span>
                    <span>
                        {fetched("status pages", run.status)} " · " {fetched("event reports", run.events)}
                        {run.nuc_status.map(|end| format!(" · NUC_STATUS through {end}"))}
                        {run.scram_dataset.map(|end| format!(" · scram dataset through {end}"))}
                        {if run.rebuilt.is_some() { " · rebuilt" } else { " · ledger unchanged" }}
                    </span>
                    {failed.then(|| view! { <span class="errors">{errors}</span> })}
                </li>
            }
        })
        .collect_view();
    view! {
        <section class="panel">
            <div class="label">"Recent runs"</div>
            <ul class="runs">{items}</ul>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn without_a_readable_build_the_panel_says_whether_the_site_still_serves() {
        assert!(pending(true).starts_with("Rebuilding after a deploy"));
        assert!(pending(false).starts_with("No build yet"));
        let status: Status = serde_json::from_str(r#"{"build":null,"runs":[]}"#).unwrap_or(Status {
            build: None,
            serving: Some("unparsed".to_owned()),
            runs: Vec::new(),
        });
        assert_eq!(status.serving, None, "a server without `serving` still parses");
    }

    #[test]
    fn timestamps_and_shares_read_plainly() {
        assert_eq!(when("2026-11-14T20:22:24.705325Z"), "2026-11-14 20:22 UTC");
        assert_eq!(when("soon"), "soon");
        assert_eq!(share(12, 714_972, 3), "0.002%");
        assert_eq!(share(1, 0, 1), "0.0%");
        assert_eq!(fetched("status pages", None), "status pages: not fetched");
        let run: Fetched = serde_json::from_str(r#"{"stored":1,"missing":0,"skipped":61}"#).unwrap_or(Fetched {
            stored: 0,
            unchanged: 9,
            missing: 0,
            skipped: 0,
        });
        assert_eq!(fetched("status pages", Some(run)), "status pages: 1 stored, 0 unchanged, 0 missing, 61 settled", "an older run parses");
        assert_eq!((thousands(0), thousands(999), thousands(1_016_840)), ("0".to_owned(), "999".to_owned(), "1,016,840".to_owned()));
    }

    #[test]
    fn the_event_archive_reads_as_pages_events_and_failures() {
        let mut archive: Archive =
            serde_json::from_str(r#"{"pages":7325,"empty":639,"events":19725,"categories":{"Agreement State":5000,"Power Reactor":9000,"Part 21":725},"failed":[],"unknown_facilities":{}}"#)
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            events(&archive),
            "7,325 Event Notification pages, 639 of them reading \"No events found\", hold 19,725 events. Every page parsed."
        );
        assert_eq!(
            by_category(&archive),
            [("Power Reactor".to_owned(), 9000), ("Agreement State".to_owned(), 5000), ("Part 21".to_owned(), 725)]
        );

        archive.failed = vec![NaiveDate::from_ymd_opt(2020, 10, 22).unwrap_or_default()];
        assert!(events(&archive).ends_with("1 page failed to parse and was skipped: 2020-10-22."), "{}", events(&archive));
        let older: Build = serde_json::from_str(r#"{"version":"v","built_at":"t","summary":{"days":1,"first":null,"last":null,"compared":0,"disagreed":0,"unknown_tags":{},"unknown_units":{},"lost_kwh":0,"without_capacity":0,"dates":{"reasons":0,"held":0,"eia":0,"missing":0}}}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(older.events, None, "a build from before the event set parses");
    }

    #[test]
    fn the_dataset_check_reads_as_one_sentence_once_a_build_has_it() {
        assert_eq!(dataset(Counts::default()), None);
        let counts: Counts = serde_json::from_str(
            r#"{"events":{"dropped":0,"named":0,"no_report":0,"low_power":0,"missed":0},"reports":{"event":0,"retracted":0,"no_scram_code":0,"missed":0},"dataset":{"first":"2011-01-01","last":"2025-05-12","agree":708,"dataset_only":9,"ours_only":8,"known":24}}"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            dataset(counts).as_deref(),
            Some(
                "Against the NRC's scram dataset, 2011-01-01 to 2025-05-12, the event reports and the dataset both list 708 of 725 scrams (97.7%), leaving aside 24 known differences: retractions, and scrams from a reactor not critical that the dataset leaves out."
            )
        );
    }

    #[test]
    fn the_scram_check_reads_as_one_sentence_once_a_build_has_it() {
        let older: Summary = serde_json::from_str(
            r#"{"days":1,"first":null,"last":null,"compared":0,"disagreed":0,"unknown_tags":{},"unknown_units":{},"lost_kwh":0,"without_capacity":0,"dates":{"reasons":0,"held":0,"eia":0,"missing":0}}"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(scrams(older.scrams), None, "a build from before the check parses and says nothing");

        let counts: Counts =
            serde_json::from_str(r#"{"events":{"dropped":1200,"named":51,"no_report":7,"low_power":121,"missed":20},"reports":{"event":1208,"retracted":8,"no_scram_code":50,"missed":529}}"#)
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            scrams(counts).as_deref(),
            Some(
                "The NRC's two sources on scrams: the status reports show 1,251 of 1,271 scrams an event report lists (98.4%), and an event report lists 1,208 of 1,787 scrams the status reports mark (67.6%)."
            )
        );
    }
}
