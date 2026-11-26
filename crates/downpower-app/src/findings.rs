//! Findings (DESIGN.md Decisions 25 and 44–47, PRD-0008): the table the build script compiles in
//! from `findings/*.md`, their pages, and each view's line of the findings drawn on it.

use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use leptos_router::hooks::{use_params_map, use_query_map};

use downpower_core::findings::Finding;

use crate::state::ViewState;
use crate::views::{View, ViewFigure};

/// Every finding, by number.
pub static FINDINGS: &[Finding] = include!(concat!(env!("OUT_DIR"), "/findings.rs"));

/// `/findings`: every finding, newest first.
#[component]
pub fn FindingsPage() -> impl IntoView {
    let items = FINDINGS
        .iter()
        .rev()
        .map(|finding| {
            view! {
                <li>
                    <a href=finding.href()>{finding.title}</a>
                    <p class="muted">{finding.summary}</p>
                    <p class="meta">{format!("{:04} · written {} · on {}", finding.number, finding.written, finding.view)}</p>
                </li>
            }
        })
        .collect_view();
    view! {
        <Title text="Findings · downpower" />
        <header class="intro">
            <h1>"Findings"</h1>
            <p class="muted">"Short notes on what the charts show, each beside its chart. A note's numbers hold as of the day it was written."</p>
        </header>
        {FINDINGS.is_empty().then(|| view! { <p class="muted">"No findings yet."</p> })}
        <ol class="findings">{items}</ol>
    }
}

/// `/findings/{NNNN-slug}`: one finding (DESIGN.md Decision 45).
#[component]
pub fn FindingPage() -> impl IntoView {
    let params = use_params_map();
    move || {
        let stem = params.with(|params| params.get("stem")).unwrap_or_default();
        match FINDINGS.iter().find(|finding| finding.stem == stem) {
            Some(finding) => view! { <Note finding /> }.into_any(),
            None => view! { <p class="muted">{format!("No finding {stem}.")}</p> }.into_any(),
        }
    }
}

/// A finding's page: its title and dates, its figure live at the pinned state, and its note. The
/// summary is the index's and the link preview's, since the note's opening says it again here.
/// Only `table` comes from the page's own URL, so the figure's table can be opened here.
#[component]
fn Note(finding: &'static Finding) -> impl IntoView {
    let query = use_query_map();
    let pinned = ViewState::parse_query(finding.query);
    let state = Memo::new(move |_| ViewState {
        table: query.with(|query| query.get("table")).as_deref() == Some("1"),
        ..pinned
    });
    let figure = View::from_path(finding.view).and_then(|view| {
        let index = view.figure(finding.figure)?;
        Some(view! { <ViewFigure view index state /> })
    });
    let table = move || if state.get().table { finding.href() } else { format!("{}?table=1", finding.href()) };
    view! {
        <Title text=format!("{} · downpower", finding.title) />
        <Meta name="description" content=finding.summary />
        <Meta property="og:title" content=finding.title />
        <Meta property="og:description" content=finding.summary />
        <Meta property="og:type" content="article" />
        <article class="finding">
            <header class="intro">
                <p class="label">
                    <a href="/findings">"Findings"</a>
                    {format!(" · {:04}", finding.number)}
                </p>
                <h1>{finding.title}</h1>
                <p class="meta">{format!("Written {}, on data through {}", finding.written, finding.through)}</p>
            </header>
            {figure}
            <div class="row wrap">
                <a class="btn" href=finding.view_href()>
                    "Open the whole view"
                </a>
                <a class="btn" aria-pressed=move || state.get().table.to_string() href=table>
                    "Table"
                </a>
            </div>
            <div class="note" inner_html=finding.html></div>
        </article>
    }
}

/// A view's line of the findings drawn on it, if any.
#[component]
pub fn FindingsOn(view: View) -> impl IntoView {
    let mine: Vec<&'static Finding> = FINDINGS.iter().filter(|finding| finding.view == view.path()).collect();
    (!mine.is_empty()).then(|| {
        let links = mine
            .into_iter()
            .enumerate()
            .map(|(index, finding)| {
                view! {
                    {(index > 0).then_some(" · ")}
                    <a href=finding.href()>{finding.title}</a>
                }
            })
            .collect_view();
        view! { <p class="meta findings-on">"Findings: " {links}</p> }
    })
}

#[cfg(test)]
mod tests {
    use downpower_core::findings::date;

    use super::*;
    use crate::state::ViewState;
    use crate::views::View;

    /// Whether a finding is written and draws a figure its view has, at a state its view reads: its
    /// query sets only the view's parameters and reads back unchanged, so a renamed or reordered
    /// parameter fails here instead of quietly drawing the default view. A stub builds, so a draft
    /// can be read locally, but can't land with its TODOs.
    fn check(finding: &Finding) -> Result<(), String> {
        let stem = finding.stem;
        if [finding.title, finding.summary, finding.html].iter().any(|text| text.contains("TODO")) {
            return Err(format!("{stem}: a TODO is left from the stub (`cargo xtask finding`); write it before it lands"));
        }
        let views = View::ALL.map(View::path).join(", ");
        let view = View::from_path(finding.view).ok_or_else(|| format!("{stem}: no chart view at {:?}; there are {views}", finding.view))?;
        let reads = view.reads();
        if let Some((name, _)) = downpower_core::explorer::pairs(finding.query).into_iter().find(|(name, _)| !reads.contains(&name.as_str())) {
            return Err(format!("{stem}: {} doesn't read `{name}`; it reads {}", finding.view, reads.join(", ")));
        }
        let canonical = ViewState::parse_query(finding.query).to_query();
        if canonical != finding.query {
            return Err(format!("{stem}: the query {:?} reads back as {canonical:?}", finding.query));
        }
        if view.figure(finding.figure).is_none() {
            let figures: Vec<&str> = view.figures().iter().map(|figure| figure.id).collect();
            return Err(format!("{stem}: {} has no figure {:?}; it has {}", finding.view, finding.figure, figures.join(", ")));
        }
        Ok(())
    }

    fn finding(view: &'static str, query: &'static str, figure: &'static str) -> Finding {
        Finding {
            number: 1,
            stem: "0001-a",
            title: "A",
            summary: "A.",
            view,
            query,
            figure,
            written: date(2026, 11, 26),
            through: date(2026, 11, 25),
            html: "<p>A.</p>\n",
        }
    }

    fn error(finding: &Finding) -> String {
        check(finding).err().unwrap_or_default()
    }

    #[test]
    fn uat_findings_catalog_every_finding_draws_a_figure_its_view_has() {
        for finding in FINDINGS {
            if let Err(error) = check(finding) {
                panic!("{error}");
            }
        }
    }

    #[test]
    fn a_finding_on_a_view_figure_at_a_state_it_reads_checks() {
        assert_eq!(check(&finding("/trends", "?from=1999-01-01", "scram-rate")), Ok(()));
        assert_eq!(check(&finding("/", "", "lost")), Ok(()));
        assert_eq!(check(&finding("/ffd", "?table=1", "ffd-rate")), Ok(()));
    }

    #[test]
    fn uat_findings_checks_reject_an_unknown_view() {
        assert!(error(&finding("/events", "", "scram-rate")).contains("/events"));
    }

    #[test]
    fn uat_findings_checks_reject_a_query_that_doesnt_round_trip() {
        let error = error(&finding("/trends", "?families=forced,grid", "scram-rate"));
        assert!(error.contains("?families=grid,forced"), "{error}");
        assert!(!self::error(&finding("/trends", "?from=1999-1-1", "scram-rate")).is_empty(), "an unparsed date reads back as nothing");
    }

    #[test]
    fn uat_findings_checks_reject_a_parameter_the_view_doesnt_read() {
        let error = error(&finding("/ffd", "?from=2010-01-01", "ffd-rate"));
        assert!(error.contains("`from`"), "{error}");
    }

    #[test]
    fn uat_findings_checks_reject_a_stub_left_unwritten() {
        let stub = Finding {
            summary: "TODO: one sentence for the index and the link preview.",
            ..finding("/trends", "", "scram-rate")
        };
        assert!(error(&stub).contains("TODO"));
        let stub = Finding {
            html: "<p>TODO: the note.</p>\n",
            ..finding("/trends", "", "scram-rate")
        };
        assert!(error(&stub).contains("TODO"));
    }

    #[test]
    fn uat_findings_checks_reject_an_unknown_figure() {
        let error = error(&finding("/trends", "", "scrams"));
        assert!(error.contains("scram-rate"), "the message lists the view's figures: {error}");
    }
}
