//! `/ffd` (PRD-0005 T-007): the fitness-for-duty Event Notifications (DESIGN.md Decisions 36 to 38).
//! Counts by year at operating reactors with Part 26's 2008 rewrite marked, the national overlay as
//! two charts on one time axis (drug-related events per 100 operating unit-years with every FFD
//! event as context, and the share of operating unit-years in states where adult use and medical
//! cannabis were legal), the state table, what the tags found, and the caveats. Descriptive only:
//! no causal estimate (Decision 38).

use std::sync::Arc;

use chrono::{Datelike as _, NaiveDate};
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use downpower_core::explorer::EventsState;
use downpower_core::ffd::{self, Kind, Role, Series, Site, State, Substance, Test, Year};
use downpower_core::trends::DAYS_PER_YEAR;

use crate::area::HEIGHT;
use crate::bars;
use crate::chart::{self, Chart, Column, Figure, Line, Mark, Table, one, percent, scale, year_label};
use crate::findings::FindingsOn;
use crate::state::ViewState;
use crate::views::View;

/// The year columns of the overlay, starred when partial.
fn columns(years: &[Year]) -> Vec<Column> {
    years
        .iter()
        .map(|year| Column {
            label: year_label(year.year, year.partial),
            partial: year.partial,
        })
        .collect()
}

/// A year's readout heading.
fn heading(year: &Year, rest: &str) -> String {
    format!("{}{} · {rest}", year.year, if year.partial { " (partial)" } else { "" })
}

/// The vertical rule at Part 26's 2008 rewrite, placed inside its year's column by the day.
fn rewrite(years: &[Year]) -> Option<Mark> {
    let date = ffd::rewrite();
    let index = years.iter().position(|year| year.year == date.year())?;
    let (x, width) = bars::column(index, years.len());
    let days = if NaiveDate::from_ymd_opt(date.year(), 2, 29).is_some() { 366.0 } else { 365.0 };
    let x = x + width * f64::from(date.ordinal0()) / days;
    Some(Mark::Line {
        class: "rule",
        path: format!("M{x:.1},0V{HEIGHT}"),
        partial: false,
    })
}

/// Stacked bars of `values` (each column's values, one per class) on a scale topping out at `top`,
/// with the rewrite's rule.
fn marks(years: &[Year], values: &[Vec<f64>], classes: &[&str], top: f64) -> Vec<Mark> {
    let series: Vec<(usize, usize)> = (0..classes.len()).map(|index| (index, index)).collect();
    let mut marks: Vec<Mark> = bars::stacked(values, &series, top)
        .into_iter()
        .map(|segment| Mark::Rect {
            class: format!("bar {}", classes[segment.key]),
            rect: segment.rect,
            partial: years.get(segment.column).is_some_and(|year| year.partial),
        })
        .collect();
    marks.extend(rewrite(years));
    marks
}

/// The scale over the tallest stack of `values`.
fn peak(values: &[Vec<f64>], unit: &str) -> (f64, Vec<(f64, String)>) {
    let indexes: Vec<usize> = (0..values.first().map_or(0, Vec::len)).collect();
    scale(bars::peak(values, &indexes), unit)
}

fn line(class: &str, value: String, label: &str) -> Line {
    Line {
        swatch: Some(format!("key {class}")),
        value,
        label: label.to_owned(),
    }
}

/// FFD events at operating reactors per year: drug-related, alcohol, and the rest.
#[must_use]
pub fn counts(years: &[Year]) -> (Chart, Table) {
    let values: Vec<Vec<f64>> = years
        .iter()
        .map(|year| [year.drug_related, year.alcohol, year.ffd - year.drug_related - year.alcohol].map(f64::from).to_vec())
        .collect();
    let (top, ticks) = peak(&values, "");
    let (columns, marks) = (columns(years), marks(years, &values, &["ffd-drug", "ffd-alcohol", "ffd-other"], top));
    let readouts = years
        .iter()
        .map(|year| {
            (
                heading(year, &format!("{} at operating reactors", year.ffd)),
                vec![
                    line("ffd-other", (year.ffd - year.drug_related - year.alcohol).to_string(), "neither named"),
                    line("ffd-alcohol", year.alcohol.to_string(), "alcohol"),
                    line("ffd-drug", year.drug_related.to_string(), "drug-related"),
                    Line {
                        swatch: None,
                        value: (year.construction + year.reactor + year.facility).to_string(),
                        label: "counted apart".to_owned(),
                    },
                ],
            )
        })
        .collect();
    let rows = years
        .iter()
        .map(|year| {
            vec![
                year_label(year.year, year.partial),
                year.ffd.to_string(),
                year.drug_related.to_string(),
                year.alcohol.to_string(),
                year.construction.to_string(),
                year.reactor.to_string(),
                year.facility.to_string(),
            ]
        })
        .collect();
    let table = Table {
        caption: "FFD events by year: at operating reactors, and counted apart".to_owned(),
        head: ["Year", "Operating", "Drug-related", "Alcohol", "Construction sites", "Other reactor sites", "Elsewhere"]
            .map(str::to_owned)
            .to_vec(),
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// Drug-related events per 100 operating unit-years, with the rest of the FFD events stacked on
/// top as context, so the bar's height is every FFD event's rate.
#[must_use]
pub fn rates(years: &[Year]) -> (Chart, Table) {
    let rate = |year: &Year, count: u32| year.rate(count).unwrap_or(0.0);
    let values: Vec<Vec<f64>> = years.iter().map(|year| vec![rate(year, year.drug_related), rate(year, year.ffd - year.drug_related)]).collect();
    let (top, ticks) = peak(&values, "");
    let (columns, marks) = (columns(years), marks(years, &values, &["ffd-drug", "ffd-rest"], top));
    let readouts = years
        .iter()
        .map(|year| {
            (
                heading(year, &format!("{} per 100 over {} unit-years", one(rate(year, year.ffd)), one(year.unit_years()))),
                vec![
                    line("ffd-rest", one(rate(year, year.ffd - year.drug_related)), "every other FFD event"),
                    line("ffd-drug", one(rate(year, year.drug_related)), "drug-related"),
                ],
            )
        })
        .collect();
    let rows = years
        .iter()
        .map(|year| {
            vec![
                year_label(year.year, year.partial),
                one(year.unit_years()),
                one(rate(year, year.drug_related)),
                one(rate(year, year.ffd)),
            ]
        })
        .collect();
    let table = Table {
        caption: "FFD events at operating reactors per 100 operating unit-years".to_owned(),
        head: ["Year", "Unit-years", "Drug-related", "All FFD"].map(str::to_owned).to_vec(),
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// The share of operating unit-years in states where adult use was legal, with the share where
/// only a comprehensive medical program was stacked on top.
#[must_use]
pub fn shares(years: &[Year]) -> (Chart, Table) {
    let split = |year: &Year| {
        let (adult, medical) = year.shares().unwrap_or((0.0, 0.0));
        (adult, (medical - adult).max(0.0), medical)
    };
    let values: Vec<Vec<f64>> = years
        .iter()
        .map(|year| {
            let (adult, medical_only, _) = split(year);
            vec![adult, medical_only]
        })
        .collect();
    // A share's scale always runs to 100%.
    let (top, ticks) = scale(100.0, "%");
    let (columns, marks) = (columns(years), marks(years, &values, &["legal-adult", "legal-medical"], top));
    let readouts = years
        .iter()
        .map(|year| {
            let (adult, medical_only, medical) = split(year);
            (
                heading(year, &format!("{} unit-years", one(year.unit_years()))),
                vec![
                    line("legal-medical", percent(medical_only), "medical only"),
                    line("legal-adult", percent(adult), "adult use"),
                    Line {
                        swatch: None,
                        value: percent(medical),
                        label: "medical, adult use or not".to_owned(),
                    },
                ],
            )
        })
        .collect();
    let rows = years
        .iter()
        .map(|year| {
            let (adult, _, medical) = split(year);
            vec![year_label(year.year, year.partial), one(year.unit_years()), percent(adult), percent(medical)]
        })
        .collect();
    let table = Table {
        caption: "Share of operating unit-years in states where cannabis was legal".to_owned(),
        head: ["Year", "Unit-years", "Adult use", "Medical"].map(str::to_owned).to_vec(),
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// `events` per 100 unit-years over `days` unit-days, or a dash without exposure.
fn per_100(events: u32, days: u32) -> String {
    if days == 0 {
        "—".to_owned()
    } else {
        one(f64::from(events) / (f64::from(days) / DAYS_PER_YEAR) * 100.0)
    }
}

/// A date, or a dash for none.
fn date(date: Option<NaiveDate>) -> String {
    date.map_or_else(|| "—".to_owned(), |date| date.to_string())
}

/// The state table's cells, the state code first.
#[must_use]
pub fn state_rows(states: &[State]) -> Vec<Vec<String>> {
    states
        .iter()
        .map(|state| {
            let law = state.law.as_ref();
            vec![
                state.state.clone(),
                one(f64::from(state.unit_days) / DAYS_PER_YEAR),
                state.ffd.to_string(),
                state.drug_related.to_string(),
                date(law.and_then(|law| law.adult_use)),
                date(law.and_then(|law| law.sales)),
                date(law.and_then(|law| law.medical)),
                state.before.0.to_string(),
                per_100(state.before.0, state.before.1),
                state.after.0.to_string(),
                per_100(state.after.0, state.after.1),
            ]
        })
        .collect()
}

/// What the tags found: per value of each dimension, the events at operating reactors and every
/// FFD event.
#[must_use]
pub fn breakdowns(series: &Series) -> Vec<Table> {
    fn table<T: Copy + PartialEq>(series: &Series, caption: &str, values: &[T], label: fn(T) -> &'static str, of: fn(&ffd::Event) -> T) -> Table {
        let rows = values
            .iter()
            .map(|value| {
                let all = series.events.iter().filter(|event| of(event) == *value);
                let (operating, total) = all.fold((0, 0), |(operating, total), event| (operating + u32::from(event.site == Site::Operating), total + 1));
                vec![label(*value).to_owned(), operating.to_string(), total.to_string()]
            })
            .collect();
        Table {
            caption: caption.to_owned(),
            head: ["", "Operating", "All"].map(str::to_owned).to_vec(),
            rows,
        }
    }
    vec![
        table(series, "Substance named", &Substance::ALL, Substance::label, |event| event.tags.substance),
        table(series, "Role", &Role::ALL, Role::label, |event| event.tags.role),
        table(series, "What happened", &Kind::ALL, Kind::label, |event| event.tags.kind),
        table(series, "Test", &Test::ALL, Test::label, |event| event.tags.test),
    ]
}

#[cfg(feature = "hydrate")]
async fn load() -> Result<Arc<Series>, String> {
    let bytes = crate::browser::fetch("/api/ffd").await?.ok_or("no FFD series yet")?;
    serde_json::from_slice(&bytes).map(Arc::new).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load() -> Result<Arc<Series>, String> {
    Err("the FFD series loads in the browser".to_owned())
}

/// The page's figures, in order (PRD-0008 T-002).
pub const FIGURES: [Figure; 3] = [
    Figure {
        id: "ffd-count",
        label: "FFD events by year",
        meta: "at operating reactors",
    },
    Figure {
        id: "ffd-rate",
        label: "Rate",
        meta: "FFD events per 100 operating unit-years",
    },
    Figure {
        id: "legal-share",
        label: "Legal cannabis",
        meta: "% of operating unit-years in states where it was legal",
    },
];

/// The series and its charts, and the loading status.
#[derive(Clone, Copy)]
struct Model {
    series: Memo<Option<Arc<Series>>>,
    charts: Memo<Option<[(Chart, Table); 3]>>,
    status: Memo<Option<String>>,
}

/// Loads the FFD series and builds its charts. They span every year: the page takes no range.
fn model() -> Model {
    let data = LocalResource::new(load);
    let series = Memo::new(move |_| data.get()?.ok());
    let charts = Memo::new(move |_| {
        let years = series.get().map(|series| ffd::years(&series))?;
        Some([counts(&years), rates(&years), shares(&years)])
    });
    let status = Memo::new(move |_| match data.get() {
        None => Some("loading…".to_owned()),
        Some(Err(error)) => Some(error),
        Some(Ok(_)) => None,
    });
    Model { series, charts, status }
}

/// A figure's legend: its swatches, and the rule marking Part 26's 2008 rewrite.
fn legend(keys: &[(&'static str, &'static str)]) -> AnyView {
    let items = keys
        .iter()
        .map(|(class, label)| {
            view! {
                <span class="legend-key">
                    <span class=format!("swatch {class}")></span>
                    {*label}
                </span>
            }
        })
        .collect_view();
    view! {
        <span class="row">
            {items}
            <span class="legend-key">
                <span class="swatch rule-key"></span>
                "Part 26 rewrite"
            </span>
        </span>
    }
    .into_any()
}

/// Figure `index` of [`FIGURES`] at `state`.
fn draw(model: Model, state: Memo<ViewState>, index: usize) -> impl IntoView {
    let keys: &[(&'static str, &'static str)] = match index {
        0 => &[("ffd-drug", "drug-related"), ("ffd-alcohol", "alcohol"), ("ffd-other", "neither named")],
        1 => &[("ffd-drug", "drug-related"), ("ffd-rest", "every other FFD event")],
        _ => &[("legal-adult", "adult use"), ("legal-medical", "medical only")],
    };
    let table = Memo::new(move |_| state.get().table);
    FIGURES.get(index).map(|spec| chart::figure(*spec, model.charts, index, table, Some(legend(keys))))
}

#[component]
pub fn FfdPage() -> impl IntoView {
    let query = use_query_map();
    let state = Memo::new(move |_| query.with(|query| ViewState::from_query(|name| query.get(name))));
    let table = Memo::new(move |_| state.get().table);
    let go = crate::filters::navigator();
    let model = model();
    let Model { series, status, .. } = model;
    let marijuana = move || {
        let series = series.get()?;
        Some(series.events.iter().filter(|event| event.tags.substance == Substance::Marijuana).count())
    };

    view! {
        <header class="intro">
            <h1>"Fitness for duty"</h1>
            <p class="muted">
                "Every Event Notification since 1999 that reports a fitness-for-duty matter: a test, a refusal, a substance found in a protected area, or a program failure. Beside them, the share of the fleet in states where cannabis was legal. The overlay describes; it doesn't estimate an effect."
            </p>
            <FindingsOn view=View::Ffd />
        </header>
        <div class="row wrap">
            <button class="btn" aria-pressed=move || table.get().to_string() on:click=move |_| go(ViewState { table: !table.get_untracked(), ..state.get_untracked() })>
                "Tables"
            </button>
            <a class="btn" href=format!("/events{}", EventsState { ffd: true, ..EventsState::default() }.to_query())>
                "List the events"
            </a>
            {move || status.get().map(|status| view! { <span class="meta">{status}</span> })}
        </div>
        {(0..FIGURES.len()).map(|index| draw(model, state, index)).collect_view()}
        <StateTable series />
        <Breakdowns series />
        <section class="intro caveats">
            <h2 class="label">"What this can and can't say"</h2>
            <p class="muted">
                "The Event Notifications carry only the FFD events 10 CFR 26.719(b) makes reportable within 24 hours: drugs or alcohol in a protected area, violations by licensed operators, supervisors and FFD program staff, and failures of the testing program. Rank-and-file positive tests reach only the annual summaries licensees file under 26.717, which the NRC publishes as totals."
            </p>
            <p class="muted">
                "An event counts by its 10 CFR section (26.73 before Part 26's 2008 rewrite, 26.719 since, 26.417(b)(1) at construction sites) or a reviewed list, never by its title. Retracted events count toward nothing. The rewrite took effect 2008-04-30 and changed what is reported, so counts on either side of the rule don't compare directly."
            </p>
            <p class="muted">
                "Substance, role, what happened and the test come from phrase rules over each event's text; \"none named\" means the text doesn't say. "
                {move || marijuana().map(|count| format!("Only {count} events name marijuana."))}
                " Since about 2021 most reports name no substance (\"failed a test specified by the fitness-for-duty testing program\"), so the drug-related and alcohol counts are floors that sink for a reason that isn't behavior, in the years adult use spreads; the all-FFD rate isn't affected. A state is the plant's, never the worker's, and a state sees one to three drug-related events a year, too few to tell an effect from noise."
            </p>
            <p class="muted">
                "Rates are per 100 operating unit-years: the ledger's in-service unit-days. Events at construction sites (Vogtle 3 and 4 before commercial operation, Summer 2 and 3), at shut-down reactors and outside power reactors count apart. Legalization dates are hand-curated from Wikipedia's timeline (CC BY-SA 4.0), checked against NCSL and the statutes; medical means a comprehensive program, as NCSL defines one."
            </p>
        </section>
    }
}

/// One of the page's figures alone, for a finding's page (DESIGN.md Decision 45).
#[component]
pub(crate) fn FfdFigure(state: Memo<ViewState>, index: usize) -> impl IntoView {
    let model = model();
    view! {
        {move || model.status.get().map(|status| view! { <p class="meta">{status}</p> })}
        {draw(model, state, index)}
    }
}

/// The state table: exposure, events and the dates, each state linking to its FFD events.
#[component]
fn StateTable(series: Memo<Option<Arc<Series>>>) -> impl IntoView {
    move || {
        let series = series.get()?;
        let rows = state_rows(&ffd::states(&series))
            .into_iter()
            .map(|row| {
                let mut cells = row.into_iter();
                let code = cells.next().unwrap_or_default();
                let href = format!(
                    "/events{}",
                    EventsState {
                        ffd: true,
                        state: Some(code.clone()),
                        ..EventsState::default()
                    }
                    .to_query()
                );
                let cells = cells.map(|cell| view! { <td>{cell}</td> }).collect_view();
                view! {
                    <tr>
                        <th scope="row">
                            <a href=href>{code}</a>
                        </th>
                        {cells}
                    </tr>
                }
            })
            .collect_view();
        Some(view! {
            <div class="table-scroll">
                <table class="totals">
                    <caption class="meta">"By state: operating unit-years, FFD events at operating reactors, the legalization dates, and drug-related events before adult use took effect and since, with their rates per 100 unit-years"</caption>
                    <thead>
                        <tr>
                            <th scope="col">"State"</th>
                            <th scope="col">"Unit-years"</th>
                            <th scope="col">"FFD"</th>
                            <th scope="col">"Drug-related"</th>
                            <th scope="col">"Adult use"</th>
                            <th scope="col">"Sales"</th>
                            <th scope="col">"Medical"</th>
                            <th scope="col">"Before"</th>
                            <th scope="col">"Per 100"</th>
                            <th scope="col">"Since"</th>
                            <th scope="col">"Per 100"</th>
                        </tr>
                    </thead>
                    <tbody>{rows}</tbody>
                </table>
            </div>
        })
    }
}

/// What the tags found, one small table per dimension.
#[component]
fn Breakdowns(series: Memo<Option<Arc<Series>>>) -> impl IntoView {
    move || {
        let series = series.get()?;
        let tables = breakdowns(&series)
            .into_iter()
            .map(|table| {
                let rows = table
                    .rows
                    .into_iter()
                    .map(|row| {
                        let mut cells = row.into_iter();
                        let label = cells.next().unwrap_or_default();
                        let cells = cells.map(|cell| view! { <td>{cell}</td> }).collect_view();
                        view! {
                            <tr>
                                <th scope="row">{label}</th>
                                {cells}
                            </tr>
                        }
                    })
                    .collect_view();
                view! {
                    <table class="totals">
                        <caption class="meta">{table.caption}</caption>
                        <thead>
                            <tr>
                                <th scope="col"></th>
                                <th scope="col">"Operating"</th>
                                <th scope="col">"All"</th>
                            </tr>
                        </thead>
                        <tbody>{rows}</tbody>
                    </table>
                }
            })
            .collect_view();
        Some(view! { <div class="breakdowns">{tables}</div> })
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn year(year: i32, partial: bool) -> Year {
        Year {
            year,
            unit_days: 36_525,
            ffd: 30,
            drug_related: 10,
            alcohol: 15,
            construction: 2,
            reactor: 1,
            facility: 0,
            adult_use_days: 9_131,
            medical_days: 18_263,
            partial,
        }
    }

    #[test]
    fn the_overlay_reads_rates_and_shares_and_marks_the_rewrite() {
        let years = [year(2007, false), year(2008, false), year(2026, true)];
        let (rate_chart, rate_table) = rates(&years);
        // 36,525 unit-days is 100 unit-years: 10 drug-related and 30 in all.
        assert_eq!(rate_table.rows[0], ["2007", "100.0", "10.0", "30.0"]);
        assert_eq!(rate_chart.readouts[2].0, "2026 (partial) · 30.0 per 100 over 100.0 unit-years");
        let rules: Vec<&Mark> = rate_chart.marks.iter().filter(|mark| matches!(mark, Mark::Line { class: "rule", .. })).collect();
        assert_eq!(rules.len(), 1, "one rule, in 2008's column");
        let (share_chart, share_table) = shares(&years);
        assert_eq!(share_table.rows[0], ["2007", "100.0", "25.00%", "50.00%"]);
        assert_eq!(share_chart.ticks.last().map(|(_, label)| label.as_str()), Some("100%"));
        let (count_chart, count_table) = counts(&years);
        assert_eq!(count_table.rows[1], ["2008", "30", "10", "15", "2", "1", "0"]);
        assert_eq!(count_chart.readouts[0].1[3].value, "3", "counted apart");
    }

    #[test]
    fn the_rewrite_has_no_rule_outside_its_year() {
        assert_eq!(rewrite(&[year(2010, false)]), None);
    }

    #[test]
    fn state_rows_split_rates_at_adult_use() {
        let state = State {
            state: "IL".to_owned(),
            unit_days: 7_305,
            ffd: 4,
            drug_related: 2,
            before: (1, 3_653),
            after: (1, 3_652),
            law: None,
        };
        assert_eq!(state_rows(&[state])[0], ["IL", "20.0", "4", "2", "—", "—", "—", "1", "10.0", "1", "10.0"]);
        assert_eq!(per_100(3, 0), "—");
    }
}
