//! `/scrams` (PRD-0007 T-006, DESIGN.md Decision 43): what started each Event Notification scram,
//! and what the scrams cost. Scrams per 100 operating unit-years by system, the energy their
//! episodes lost as a share of net capacity, the scrams tied to testing, maintenance or an error,
//! and a unit table. The page reads the home page's `ViewState` range, so a link carries across,
//! and `table=1` opens every chart's table.
//!
//! Each chart is built natively into a [`Chart`] and a [`Table`] from the data, as the trends
//! page's are, so what it shows is tested without a browser.

use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use downpower_core::explorer::EventsState;
use downpower_core::trends::Trends;
use downpower_core::triggers::{self, Circumstance, Series, System, UnitRow, Year};

use crate::bars;
use crate::chart::{Chart, Column, Line, Mark, Plot, Table, TableView, one, percent, scale, year_label};
use crate::filters::Range;
use crate::state::ViewState;

/// A system's CSS class suffix: its slug with hyphens.
fn css(system: System) -> String {
    system.slug().replace('_', "-")
}

/// The year columns, starred where the days shown leave part of the year out.
fn columns(years: &[Year]) -> Vec<Column> {
    years
        .iter()
        .map(|year| Column {
            label: year_label(year.year, year.partial),
            partial: year.partial,
        })
        .collect()
}

/// A readout's heading: the year, flagged partial.
fn heading(year: &Year) -> String {
    format!("{}{}", year.year, if year.partial { " (partial)" } else { "" })
}

/// Bars of `values` stacked by system in [`System::ALL`]'s order.
fn stacked(columns: &[Column], values: &[[f64; 8]], top: f64) -> Vec<Mark> {
    let series: Vec<(System, usize)> = System::ALL.into_iter().map(|system| (system, system.index())).collect();
    bars::stacked(values, &series, top)
        .into_iter()
        .map(|segment| Mark::Rect {
            class: format!("bar system-{}", css(segment.key)),
            rect: segment.rect,
            partial: columns.get(segment.column).is_some_and(|column| column.partial),
        })
        .collect()
}

/// Scrams of each system per 100 operating unit-years, stacked in [`System::ALL`]'s order.
#[must_use]
pub fn trigger_rate(years: &[Year]) -> (Chart, Table) {
    let columns = columns(years);
    let rates: Vec<[f64; 8]> = years.iter().map(|year| System::ALL.map(|system| year.rate(year.scrams[system.index()]).unwrap_or(0.0))).collect();
    let (top, ticks) = scale(bars::peak(&rates, &(0..8).collect::<Vec<_>>()), "");
    let marks = stacked(&columns, &rates, top);
    let readouts = years
        .iter()
        .zip(&rates)
        .map(|(year, rate)| {
            let lines = System::ALL
                .into_iter()
                .rev()
                .filter(|system| year.scrams[system.index()] > 0)
                .map(|system| Line {
                    swatch: Some(format!("key system-{}", css(system))),
                    value: one(rate[system.index()]),
                    label: format!("{} ({})", system.label(), year.scrams[system.index()]),
                })
                .collect();
            let total = year.rate(year.total()).unwrap_or(0.0);
            (format!("{} · {} per 100 unit-years over {}", heading(year), one(total), one(year.unit_years())), lines)
        })
        .collect();
    let mut head = ["Year", "Unit-years", "Scrams", "Per 100"].map(str::to_owned).to_vec();
    head.extend(System::ALL.map(|system| system.label().to_owned()));
    let rows = years
        .iter()
        .map(|year| {
            let mut row = vec![
                year_label(year.year, year.partial),
                one(year.unit_years()),
                year.total().to_string(),
                one(year.rate(year.total()).unwrap_or(0.0)),
            ];
            row.extend(System::ALL.map(|system| year.scrams[system.index()].to_string()));
            row
        })
        .collect();
    let table = Table {
        caption: "Event Notification scrams by the system the report names, and per 100 operating unit-years".to_owned(),
        head,
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// The energy each system's episodes lost, as a share of the year's net capacity energy.
#[must_use]
pub fn trigger_loss(years: &[Year]) -> (Chart, Table) {
    let columns = columns(years);
    let shares: Vec<[f64; 8]> = years.iter().map(|year| System::ALL.map(|system| year.share(system).unwrap_or(0.0))).collect();
    let (top, ticks) = scale(bars::peak(&shares, &(0..8).collect::<Vec<_>>()), "%");
    let marks = stacked(&columns, &shares, top);
    let readouts = years
        .iter()
        .zip(&shares)
        .map(|(year, share)| {
            let lines = System::ALL
                .into_iter()
                .rev()
                .filter(|system| year.lost_kwh[system.index()] > 0)
                .map(|system| Line {
                    swatch: Some(format!("key system-{}", css(system))),
                    value: percent(share[system.index()]),
                    label: system.label().to_owned(),
                })
                .collect();
            (format!("{} · {} of capacity", heading(year), percent(share.iter().sum())), lines)
        })
        .collect();
    let mut head = ["Year", "All"].map(str::to_owned).to_vec();
    head.extend(System::ALL.map(|system| system.label().to_owned()));
    let rows = years
        .iter()
        .zip(&shares)
        .map(|(year, share)| {
            let mut row = vec![year_label(year.year, year.partial), percent(share.iter().sum())];
            row.extend(System::ALL.map(|system| percent(share[system.index()])));
            row
        })
        .collect();
    let table = Table {
        caption: "Output lost in scram episodes as a share of net capacity, by system, in the year each scram happened".to_owned(),
        head,
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// The circumstances the tied chart counts, and weather, which its readout and table add.
const TIED: [Circumstance; 3] = [Circumstance::Testing, Circumstance::Maintenance, Circumstance::HumanError];

/// Scrams the text ties to testing, maintenance or an error, per 100 operating unit-years.
#[must_use]
pub fn tied_rate(years: &[Year]) -> (Chart, Table) {
    let columns = columns(years);
    let tied = |year: &Year| TIED.iter().map(|circumstance| year.circumstances[circumstance.index()]).sum::<u32>();
    let rates: Vec<[f64; 1]> = years.iter().map(|year| [year.rate(tied(year)).unwrap_or(0.0)]).collect();
    let (top, ticks) = scale(bars::peak(&rates, &[0]), "");
    let marks = bars::stacked(&rates, &[((), 0)], top)
        .into_iter()
        .map(|segment| Mark::Rect {
            class: "bar tied".to_owned(),
            rect: segment.rect,
            partial: columns.get(segment.column).is_some_and(|column| column.partial),
        })
        .collect();
    let readouts = years
        .iter()
        .zip(&rates)
        .map(|(year, rate)| {
            let lines = TIED
                .iter()
                .chain(&[Circumstance::Weather])
                .map(|circumstance| {
                    let count = year.circumstances[circumstance.index()];
                    Line {
                        swatch: None,
                        value: one(year.rate(count).unwrap_or(0.0)),
                        label: format!("{} ({count})", circumstance.label()),
                    }
                })
                .collect();
            (format!("{} · {} per 100 unit-years tied to testing, maintenance or an error", heading(year), one(rate[0])), lines)
        })
        .collect();
    let rows = years
        .iter()
        .zip(&rates)
        .map(|(year, rate)| {
            let mut row = vec![year_label(year.year, year.partial), one(year.unit_years())];
            row.extend(TIED.iter().chain(&[Circumstance::Weather]).map(|circumstance| year.circumstances[circumstance.index()].to_string()));
            row.push(one(rate[0]));
            row
        })
        .collect();
    let table = Table {
        caption: "Scrams by what the report ties them to, and those tied to testing, maintenance or an error per 100 operating unit-years".to_owned(),
        head: ["Year", "Unit-years", "Testing", "Maintenance", "Human error", "Weather", "Tied per 100"].map(str::to_owned).to_vec(),
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// Unit-days a unit needs over the range to get a row: a year, since a rate over less is noise.
const MIN_UNIT_DAYS: u32 = 365;

/// The unit table's rows: units with a year or more in service over the range, most scrams per
/// unit-year first.
#[must_use]
pub fn ranked(units: Vec<UnitRow>) -> Vec<UnitRow> {
    let mut rows: Vec<UnitRow> = units.into_iter().filter(|row| row.unit_days >= MIN_UNIT_DAYS).collect();
    // Compares scrams over unit-days by cross-multiplying, exact in u64.
    rows.sort_by(|a, b| {
        (u64::from(b.scrams) * u64::from(a.unit_days))
            .cmp(&(u64::from(a.scrams) * u64::from(b.unit_days)))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

/// `count` per 100 unit-years over `unit_days`.
fn per_100(count: u32, unit_days: u32) -> String {
    if unit_days == 0 {
        return "–".to_owned();
    }
    one(f64::from(count) / (f64::from(unit_days) / downpower_core::trends::DAYS_PER_YEAR) * 100.0)
}

#[cfg(feature = "hydrate")]
async fn load_series() -> Result<Arc<Series>, String> {
    let bytes = crate::browser::fetch("/api/scrams").await?.ok_or("no scram series yet")?;
    serde_json::from_slice(&bytes).map(Arc::new).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load_series() -> Result<Arc<Series>, String> {
    Err("the scram series loads in the browser".to_owned())
}

/// The systems' legend, in the stack's order from the top.
fn legend() -> AnyView {
    let items = System::ALL
        .into_iter()
        .rev()
        .map(|system| {
            view! {
                <span class="legend-key">
                    <span class=format!("swatch system-{}", css(system))></span>
                    {system.label()}
                </span>
            }
        })
        .collect_view();
    view! { <span class="row legend-keys">{items}</span> }.into_any()
}

#[component]
pub fn ScramsPage() -> impl IntoView {
    let query = use_query_map();
    let state = Memo::new(move |_| query.with(|query| ViewState::from_query(|name| query.get(name))));
    let trends = LocalResource::new(crate::trends::load_trends);
    let series = LocalResource::new(load_series);
    let data = Memo::new(move |_| -> Option<(Arc<Trends>, Arc<Series>)> { Some((trends.get()?.ok()?, series.get()?.ok()?)) });
    let years = Memo::new(move |_| {
        let (trends, series) = data.get()?;
        let view = state.get();
        Some(triggers::years(&series, &trends, view.from, view.to))
    });
    let charts = Memo::new(move |_| {
        let years = years.get()?;
        Some([trigger_rate(&years), trigger_loss(&years), tied_rate(&years)])
    });
    let status = move || match (trends.get(), series.get()) {
        (Some(Err(error)), _) | (_, Some(Err(error))) => Some(error),
        (Some(Ok(_)), Some(Ok(_))) => None,
        _ => Some("loading…".to_owned()),
    };
    let note = move || {
        let last = data.get().and_then(|(trends, _)| trends.last)?;
        Some(format!("* partial: the range or the data covers part of the year. The data runs through {last}."))
    };
    let go = crate::filters::navigator();
    let table = Memo::new(move |_| state.get().table);
    let figure = move |index: usize, label: &'static str, meta: &'static str, legend: Option<AnyView>| {
        let chart = Memo::new(move |_| charts.get().map(|charts| charts[index].0.clone()));
        let rows = Memo::new(move |_| charts.get().map(|charts| charts[index].1.clone()));
        view! {
            <figure class="area bars">
                <figcaption class="row">
                    <span class="label">{label}</span>
                    <span class="meta">{meta}</span>
                    {legend}
                </figcaption>
                <Plot chart />
                {move || table.get().then(|| view! { <TableView table=rows /> })}
            </figure>
        }
    };
    view! {
        <header class="intro">
            <h1>"Scram triggers"</h1>
            <p class="muted">
                "What started each reactor scram the NRC was notified of, and what the scrams cost: the output each unit lost until it was back near the power it scrammed from. The range above drives every chart."
            </p>
        </header>
        <div class="filters">
            <Range state />
        </div>
        <div class="row">
            <button class="btn" aria-pressed=move || table.get().to_string() on:click=move |_| go(ViewState { table: !table.get_untracked(), ..state.get_untracked() })>
                "Tables"
            </button>
            {move || status().map(|status| view! { <span class="meta">{status}</span> })}
            {move || note().map(|note| view! { <span class="meta">{note}</span> })}
        </div>
        {figure(0, "Scrams by trigger", "per 100 operating unit-years, by the system the report says started the scram", Some(legend()))}
        {figure(1, "Output lost to scrams", "% of net capacity, each scram's episode counted in the year it began", Some(legend()))}
        {figure(2, "Scrams tied to testing, maintenance or an error", "per 100 operating unit-years; the readout and table add weather", None)}
        <UnitTable data state years />
        <Caveats />
    }
}

/// The unit table: each unit's scrams over the range, linked to its scrams in the explorer.
#[component]
fn UnitTable(data: Memo<Option<(Arc<Trends>, Arc<Series>)>>, state: Memo<ViewState>, years: Memo<Option<Vec<Year>>>) -> impl IntoView {
    let units = move || {
        let (_, series) = data.get()?;
        let view = state.get();
        let years = years.get()?;
        let fleet: (u32, u32) = years.iter().fold((0, 0), |(scrams, days), year| (scrams + year.total(), days + year.unit_days));
        let rows = ranked(triggers::units(&series, view.from, view.to))
            .into_iter()
            .map(|row| {
                let href = format!(
                    "/events{}",
                    EventsState {
                        from: view.from,
                        to: view.to,
                        unit: Some(row.docket.clone()),
                        scram: true,
                        ..EventsState::default()
                    }
                    .to_query()
                );
                let leading = row.leading.map(|system| {
                    view! {
                        <span class="legend-key">
                            <span class=format!("swatch system-{}", css(system))></span>
                            {system.label()}
                        </span>
                    }
                });
                view! {
                    <tr>
                        <th scope="row"><a href=href>{row.name.clone()}</a></th>
                        <td>{one(f64::from(row.unit_days) / downpower_core::trends::DAYS_PER_YEAR)}</td>
                        <td>{row.scrams}</td>
                        <td>{per_100(row.scrams, row.unit_days)}</td>
                        <td>{row.median_days.map_or_else(|| "–".to_owned(), one)}</td>
                        <td>{(row.lost_kwh + 500_000) / 1_000_000}</td>
                        <td>{leading}</td>
                    </tr>
                }
            })
            .collect_view();
        Some(view! {
            <div class="table-scroll">
                <table class="totals">
                    <caption class="meta">
                        {format!(
                            "Units with a year or more in service over the range, most scrams per unit-year first. The fleet: {} per 100 unit-years.",
                            per_100(fleet.0, fleet.1)
                        )}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">"Unit"</th>
                            <th scope="col">"Unit-years"</th>
                            <th scope="col">"Scrams"</th>
                            <th scope="col">"Per 100"</th>
                            <th scope="col">"Median days down"</th>
                            <th scope="col">"Lost GWh"</th>
                            <th scope="col">"Leading trigger"</th>
                        </tr>
                    </thead>
                    <tbody>{rows}</tbody>
                </table>
            </div>
        })
    };

    view! {
        <section class="units">
            <div class="label">"By unit"</div>
            {units}
        </section>
    }
}

/// What the tags and the episodes are, and how far to trust them.
#[component]
fn Caveats() -> impl IntoView {
    view! {
        <section class="caveats muted">
            <p>
                "A scram's system is what its Event Notification says started it, read by phrase rules: the most upstream cause the text names, and when it names only the trip signal, the signal's system, so a turbine trip with no cause given counts under the turbine. 40% of the reports leave the root cause under investigation. On 50 scrams the rules were never tuned on, they agreed with a hand label on the system for 39 and on the circumstance for 43."
            </p>
            <p>
                "A scram's episode runs from the day the status reports show the drop until the unit is back within 10 points of the power it scrammed from, whatever cause each day is attributed to; a refueling outage or the unit's next scram ends it early. Its energy is measured below the pre-scram power and counted in the year the scram happened, against the fleet's net capacity that year. Rates are per 100 operating unit-years, the ledger's in-service unit-days."
            </p>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::chart::Mark;

    /// A year of 10.0 unit-years and 1,000 kWh of capacity.
    fn year(partial: bool) -> Year {
        let mut scrams = [0; 8];
        scrams[System::Feedwater.index()] = 4;
        scrams[System::TurbineGenerator.index()] = 6;
        let mut lost_kwh = [0; 8];
        lost_kwh[System::Feedwater.index()] = 30;
        lost_kwh[System::TurbineGenerator.index()] = 10;
        let mut circumstances = [0; 5];
        circumstances[Circumstance::Testing.index()] = 2;
        circumstances[Circumstance::Maintenance.index()] = 1;
        circumstances[Circumstance::HumanError.index()] = 1;
        circumstances[Circumstance::Weather.index()] = 3;
        circumstances[Circumstance::Unnamed.index()] = 3;
        Year {
            year: 2024,
            unit_days: 3_653,
            capacity_kwh: 1_000,
            scrams,
            circumstances,
            lost_kwh,
            partial,
        }
    }

    #[test]
    fn the_rate_chart_stacks_the_systems_per_100_unit_years() {
        let (chart, table) = trigger_rate(&[year(false)]);
        assert_eq!(chart.readouts[0].0, "2024 · 100.0 per 100 unit-years over 10.0");
        let lines: Vec<(&str, &str)> = chart.readouts[0].1.iter().map(|line| (line.label.as_str(), line.value.as_str())).collect();
        assert_eq!(
            lines,
            [("Turbine and generator (6)", "60.0"), ("Feedwater and condensate (4)", "40.0")],
            "named systems, top of the stack first"
        );
        assert!(chart.marks.iter().all(|mark| matches!(mark, Mark::Rect { class, .. } if class.starts_with("bar system-"))));
        assert_eq!(table.head[..4], ["Year", "Unit-years", "Scrams", "Per 100"]);
        assert_eq!(table.rows[0][..5], ["2024", "10.0", "10", "100.0", "4"]);
    }

    #[test]
    fn the_loss_chart_reads_shares_of_capacity_and_stars_a_partial_year() {
        let (chart, table) = trigger_loss(&[year(true)]);
        assert_eq!(chart.columns[0].label, "2024*");
        assert_eq!(chart.readouts[0].0, "2024 (partial) · 4.00% of capacity");
        assert_eq!(chart.readouts[0].1.last().map(|line| line.value.as_str()), Some("3.00%"), "feedwater at the bottom");
        assert_eq!(table.rows[0][..3], ["2024*", "4.00%", "3.00%"]);
    }

    #[test]
    fn the_tied_chart_counts_testing_maintenance_and_errors_and_reads_out_weather() {
        let (chart, table) = tied_rate(&[year(false)]);
        assert_eq!(chart.readouts[0].0, "2024 · 40.0 per 100 unit-years tied to testing, maintenance or an error");
        let labels: Vec<&str> = chart.readouts[0].1.iter().map(|line| line.label.as_str()).collect();
        assert_eq!(labels, ["Testing (2)", "Maintenance (1)", "Human error (1)", "Weather (3)"]);
        assert_eq!(chart.marks.len(), 1);
        assert_eq!(table.rows[0], ["2024", "10.0", "2", "1", "1", "3", "40.0"]);
    }

    fn unit(docket: &str, unit_days: u32, scrams: u32) -> UnitRow {
        UnitRow {
            docket: docket.to_owned(),
            name: docket.to_owned(),
            unit_days,
            scrams,
            systems: [0; 8],
            median_days: None,
            lost_kwh: 0,
            leading: None,
        }
    }

    #[test]
    fn the_unit_table_ranks_by_rate_and_drops_units_with_under_a_year() {
        let rows = ranked(vec![unit("A", 3_653, 5), unit("B", 1_000, 4), unit("C", 200, 3)]);
        assert_eq!(rows.iter().map(|row| row.docket.as_str()).collect::<Vec<_>>(), ["B", "A"]);
    }
}
