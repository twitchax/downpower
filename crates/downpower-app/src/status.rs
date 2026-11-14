//! `/status`: what the ledger rests on (PRD-0002 T-015). The current build's coverage by source,
//! the NRC/EIA disagreement rate, what the classifier quarantined, and the update loop's recent
//! runs. The types mirror the subset of `/api/status` shown here; the server's tests parse its
//! output into them, so the two can't drift apart.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use leptos::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Status {
    pub build: Option<Build>,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Build {
    pub version: String,
    pub built_at: String,
    pub summary: Summary,
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
    pub rebuilt: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Fetched {
    pub stored: u64,
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

/// A fetch summary in words.
fn fetched(label: &str, fetched: Option<Fetched>) -> String {
    fetched.map_or_else(
        || format!("{label}: not fetched"),
        |fetched| format!("{label}: {} stored, {} missing, {} settled", fetched.stored, fetched.missing, fetched.skipped),
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
                <Ledger current=status.build />
                <Runs runs=status.runs />
            }
            .into_any(),
        }}
    }
}

/// The current build: coverage, agreement and quarantine.
#[component]
fn Ledger(current: Option<Build>) -> impl IntoView {
    let Some(build) = current else {
        return view! { <section class="panel"><p class="muted">"No build yet: the first one takes a few minutes after a deploy."</p></section> }.into_any();
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
    fn timestamps_and_shares_read_plainly() {
        assert_eq!(when("2026-11-14T20:22:24.705325Z"), "2026-11-14 20:22 UTC");
        assert_eq!(when("soon"), "soon");
        assert_eq!(share(12, 714_972, 3), "0.002%");
        assert_eq!(share(1, 0, 1), "0.0%");
        assert_eq!(fetched("status pages", None), "status pages: not fetched");
        assert_eq!((thousands(0), thousands(999), thousands(1_016_840)), ("0".to_owned(), "999".to_owned(), "1,016,840".to_owned()));
    }
}
