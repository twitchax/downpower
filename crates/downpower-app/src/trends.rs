//! `/trends` (PRD-0006): the fleet over the years in four charts, each a rate so a fleet that
//! changes size doesn't skew it. Reported power reduction as a share of net capacity by year and by
//! calendar month, refueling outage length by year, and Event Notification scrams per 100
//! operating unit-years. The page reads the home page's `ViewState`: the range picks the months,
//! the layer and the families drive the two loss charts, and `table=1` opens every chart's table.
//!
//! Each chart is built natively into a [`Chart`] and a [`Table`] from the data, so what it shows
//! is tested without a browser; [`Plot`] and [`TableView`] only place them.

use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use downpower_core::series::Monthly;
use downpower_core::trends::{self as fleet, OutageYear, ScramKind, Season, Trends, Year};
use downpower_core::{Families, Family};

use crate::bars;
use crate::chart::{Chart, Column, Line, Mark, Plot, Table, TableView, one, percent, scale, year_label};
use crate::filters::Filters;
use crate::state::ViewState;

/// The visible families bottom up, each with its index in a column of eight.
fn stack(visible: Families) -> Vec<(Family, usize)> {
    Family::STACK
        .into_iter()
        .filter(|family| visible.contains(*family))
        .map(|family| (family, usize::from(family.index())))
        .collect()
}

/// Columns of shares by family, stacked, with each column's readout and total.
fn family_chart(columns: Vec<Column>, shares: &[[f64; 8]], visible: Families, headings: Vec<String>) -> Chart {
    let series = stack(visible);
    let indexes: Vec<usize> = series.iter().map(|(_, index)| *index).collect();
    let (top, ticks) = scale(bars::peak(shares, &indexes), "%");
    let marks = bars::stacked(shares, &series, top)
        .into_iter()
        .map(|segment| Mark::Rect {
            class: format!("bar family-{}", segment.key.slug()),
            rect: segment.rect,
            partial: columns.get(segment.column).is_some_and(|column| column.partial),
        })
        .collect();
    let readouts = shares
        .iter()
        .zip(headings)
        .map(|(column, heading)| {
            let total: f64 = indexes.iter().map(|index| column[*index]).sum();
            let lines = series
                .iter()
                .rev()
                .map(|(family, index)| Line {
                    swatch: Some(format!("key family-{}", family.slug())),
                    value: percent(column[*index]),
                    label: family.label().to_owned(),
                })
                .collect();
            (format!("{heading} · {} of capacity", percent(total)), lines)
        })
        .collect();
    Chart { columns, top, ticks, marks, readouts }
}

/// Each family's share of a year's or a calendar month's net capacity, zero without capacity.
fn shares(share: impl Fn(Family) -> Option<f64>) -> [f64; 8] {
    Family::ALL.map(|family| share(family).unwrap_or(0.0))
}

/// The family table: a row per column, its total and each visible family's share.
fn family_table(caption: String, first: &str, labels: &[String], shares: &[[f64; 8]], visible: Families) -> Table {
    let series = stack(visible);
    let mut head = vec![first.to_owned(), "All shown".to_owned()];
    head.extend(series.iter().rev().map(|(family, _)| family.label().to_owned()));
    let rows = labels
        .iter()
        .zip(shares)
        .map(|(label, column)| {
            let total: f64 = series.iter().map(|(_, index)| column[*index]).sum();
            let mut row = vec![label.clone(), percent(total)];
            row.extend(series.iter().rev().map(|(_, index)| percent(column[*index])));
            row
        })
        .collect();
    Table { caption, head, rows }
}

/// Reported power reduction as a share of net capacity, per year, by family.
#[must_use]
pub fn loss_by_year(years: &[Year], visible: Families) -> (Chart, Table) {
    let columns: Vec<Column> = years
        .iter()
        .map(|year| Column {
            label: year_label(year.year, year.partial),
            partial: year.partial,
        })
        .collect();
    let values: Vec<[f64; 8]> = years.iter().map(|year| shares(|family| year.share(family))).collect();
    let labels: Vec<String> = columns.iter().map(|column| column.label.clone()).collect();
    let table = family_table("Reported power reduction as a share of net capacity, by year".to_owned(), "Year", &labels, &values, visible);
    let headings = years.iter().map(|year| format!("{}{}", year.year, if year.partial { " (partial)" } else { "" })).collect();
    (family_chart(columns, &values, visible, headings), table)
}

/// The three-letter names of the months.
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The same share by calendar month, across the range's years.
#[must_use]
pub fn loss_by_month(seasons: &[Season; 12], visible: Families) -> (Chart, Table) {
    let columns: Vec<Column> = MONTHS
        .iter()
        .map(|month| Column {
            label: (*month).to_owned(),
            partial: false,
        })
        .collect();
    let values: Vec<[f64; 8]> = seasons.iter().map(|season| shares(|family| season.share(family))).collect();
    let labels: Vec<String> = MONTHS.iter().zip(seasons).map(|(month, season)| format!("{month} ({})", season.months)).collect();
    let table = family_table(
        "Reported power reduction as a share of net capacity, by calendar month (months averaged)".to_owned(),
        "Month",
        &labels,
        &values,
        visible,
    );
    let headings = MONTHS.iter().zip(seasons).map(|(month, season)| format!("{month}, {} months", season.months)).collect();
    (family_chart(columns, &values, visible, headings), table)
}

/// Refueling outage length per year: the middle half as a box and the median as a line across it.
#[must_use]
pub fn outage_length(years: &[OutageYear]) -> (Chart, Table) {
    let columns: Vec<Column> = years
        .iter()
        .map(|year| Column {
            label: year_label(year.year, year.partial),
            partial: year.partial,
        })
        .collect();
    let (top, ticks) = scale(years.iter().map(|year| year.quartiles.1).fold(0.0, f64::max), "");
    let mut marks = Vec::new();
    let mut readouts = Vec::new();
    let mut rows = Vec::new();
    for (index, year) in years.iter().enumerate() {
        let (rect, line) = bars::spread(index, years.len(), year.quartiles, year.median, top);
        marks.push(Mark::Rect {
            class: "spread".to_owned(),
            rect,
            partial: year.partial,
        });
        marks.push(Mark::Line {
            class: "median",
            path: line,
            partial: year.partial,
        });
        let days = |value: f64| format!("{} days", one(value));
        readouts.push((
            format!("{}{} · {} outages", year.year, if year.partial { " (partial)" } else { "" }, year.count),
            vec![
                Line {
                    swatch: Some("key median-key".to_owned()),
                    value: days(year.median),
                    label: "median".to_owned(),
                },
                Line {
                    swatch: Some("key spread-key".to_owned()),
                    value: format!("{}–{}", one(year.quartiles.0), days(year.quartiles.1)),
                    label: "middle half".to_owned(),
                },
                Line {
                    swatch: None,
                    value: format!("{}–{} days", year.shortest, year.longest),
                    label: "shortest to longest".to_owned(),
                },
            ],
        ));
        rows.push(vec![
            year_label(year.year, year.partial),
            year.count.to_string(),
            one(year.median),
            one(year.quartiles.0),
            one(year.quartiles.1),
            year.shortest.to_string(),
            year.longest.to_string(),
        ]);
    }
    let table = Table {
        caption: "Refueling outages by the year they began: days at 0% power".to_owned(),
        head: ["Year", "Outages", "Median", "First quartile", "Third quartile", "Shortest", "Longest"].map(str::to_owned).to_vec(),
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

/// EN scrams per 100 operating unit-years, automatic stacked under manual.
#[must_use]
pub fn scram_rate(years: &[Year]) -> (Chart, Table) {
    let columns: Vec<Column> = years
        .iter()
        .map(|year| Column {
            label: year_label(year.year, year.partial),
            partial: year.partial,
        })
        .collect();
    let rates: Vec<[f64; 2]> = years.iter().map(|year| ScramKind::ALL.map(|kind| year.scram_rate(kind).unwrap_or(0.0))).collect();
    let series: Vec<(ScramKind, usize)> = ScramKind::ALL.into_iter().map(|kind| (kind, kind.index())).collect();
    let (top, ticks) = scale(bars::peak(&rates, &[0, 1]), "");
    let marks = bars::stacked(&rates, &series, top)
        .into_iter()
        .map(|segment| Mark::Rect {
            class: format!("bar kind-{}", segment.key.slug()),
            rect: segment.rect,
            partial: columns.get(segment.column).is_some_and(|column| column.partial),
        })
        .collect();
    let readouts = years
        .iter()
        .zip(&rates)
        .map(|(year, rate)| {
            let lines = ScramKind::ALL
                .into_iter()
                .rev()
                .map(|kind| Line {
                    swatch: Some(format!("key kind-{}", kind.slug())),
                    value: one(rate[kind.index()]),
                    label: format!("{} ({})", kind.label(), year.scrams[kind.index()]),
                })
                .collect();
            (
                format!(
                    "{}{} · {} per 100 unit-years over {}",
                    year.year,
                    if year.partial { " (partial)" } else { "" },
                    one(rate.iter().sum()),
                    one(year.unit_years())
                ),
                lines,
            )
        })
        .collect();
    let rows = years
        .iter()
        .zip(&rates)
        .map(|(year, rate)| {
            vec![
                year_label(year.year, year.partial),
                one(year.unit_years()),
                year.scrams[0].to_string(),
                year.scrams[1].to_string(),
                one(rate[0]),
                one(rate[1]),
            ]
        })
        .collect();
    let table = Table {
        caption: "Event Notification scrams, and per 100 operating unit-years".to_owned(),
        head: ["Year", "Unit-years", "Automatic", "Manual", "Automatic per 100", "Manual per 100"].map(str::to_owned).to_vec(),
        rows,
    };
    (Chart { columns, top, ticks, marks, readouts }, table)
}

#[cfg(feature = "hydrate")]
async fn load_trends() -> Result<Arc<Trends>, String> {
    let bytes = crate::browser::fetch("/api/trends").await?.ok_or("no trends yet")?;
    serde_json::from_slice(&bytes).map(Arc::new).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load_trends() -> Result<Arc<Trends>, String> {
    Err("the trends load in the browser".to_owned())
}

#[component]
pub fn TrendsPage() -> impl IntoView {
    let query = use_query_map();
    let state = Memo::new(move |_| query.with(|query| ViewState::from_query(|name| query.get(name))));
    let layer = Memo::new(move |_| state.get().layer);
    let monthly = LocalResource::new(move || crate::lost::load(layer.get()));
    let trends = LocalResource::new(load_trends);
    let data = Memo::new(move |_| -> Option<(Arc<Trends>, Arc<Monthly>)> { Some((trends.get()?.ok()?, monthly.get()?.ok()?)) });
    let years = Memo::new(move |_| {
        let (trends, monthly) = data.get()?;
        let view = state.get();
        Some(fleet::years(&trends, &monthly, view.from, view.to))
    });
    let charts = Memo::new(move |_| {
        let (trends, monthly) = data.get()?;
        let view = state.get();
        let years = years.get()?;
        let seasons = fleet::seasons(&trends, &monthly, view.from, view.to);
        let outages = fleet::outage_years(&trends, view.from, view.to);
        Some([loss_by_year(&years, view.families), loss_by_month(&seasons, view.families), outage_length(&outages), scram_rate(&years)])
    });
    let status = move || match (trends.get(), monthly.get()) {
        (Some(Err(error)), _) | (_, Some(Err(error))) => Some(error),
        (Some(Ok(_)), Some(Ok(_))) => None,
        _ => Some("loading…".to_owned()),
    };
    let note = move || {
        let last = data.get().and_then(|(trends, _)| trends.last)?;
        Some(format!(
            "* partial: the range or the data covers part of the year. The data runs through {last}, and the last 28 days await their reasons."
        ))
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
    let kinds = || {
        let items = ScramKind::ALL
            .into_iter()
            .map(|kind| {
                view! {
                    <span class="legend-key">
                        <span class=format!("swatch kind-{}", kind.slug())></span>
                        {kind.label()}
                    </span>
                }
            })
            .collect_view();
        view! { <span class="row">{items}</span> }.into_any()
    };
    let spread = || {
        view! {
            <span class="row">
                <span class="legend-key">
                    <span class="swatch spread-key"></span>
                    "middle half"
                </span>
                <span class="legend-key">
                    <span class="swatch median-key"></span>
                    "median"
                </span>
            </span>
        }
        .into_any()
    };

    view! {
        <header class="intro">
            <h1>"Fleet trends"</h1>
            <p class="muted">
                "The fleet over the years, each chart a rate so a fleet that changes size doesn't skew it. The range, layer and families above drive every chart."
            </p>
        </header>
        <Filters state markers=false />
        <div class="row">
            <button class="btn" aria-pressed=move || table.get().to_string() on:click=move |_| go(ViewState { table: !table.get_untracked(), ..state.get_untracked() })>
                "Tables"
            </button>
            {move || status().map(|status| view! { <span class="meta">{status}</span> })}
            {move || note().map(|note| view! { <span class="meta">{note}</span> })}
        </div>
        {figure(0, "Reported power reduction by year", "% of net capacity, by cause family", None)}
        {figure(1, "By month of the year", "% of net capacity, each calendar month across the range", None)}
        {figure(2, "Refueling outage length", "days at 0% power, by the year the outage began", Some(spread()))}
        {figure(3, "Scrams", "Event Notification scrams per 100 operating unit-years", Some(kinds()))}
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn year(year: i32, partial: bool) -> Year {
        let mut lost_kwh = [0; 8];
        lost_kwh[usize::from(Family::Refueling.index())] = 60;
        lost_kwh[usize::from(Family::Forced.index())] = 20;
        Year {
            year,
            unit_days: 3_653,
            capacity_kwh: 1_000,
            lost_kwh,
            scrams: [20, 5],
            partial,
        }
    }

    #[test]
    fn the_loss_chart_stacks_the_visible_families_and_stars_a_partial_year() {
        let years = [year(2024, false), year(2025, true)];
        let (chart, table) = loss_by_year(&years, Families::ALL.toggled(Family::Forced));
        assert_eq!(chart.columns.iter().map(|column| column.label.as_str()).collect::<Vec<_>>(), ["2024", "2025*"]);
        // Forced is hidden: one Refueling bar per year, the second drawn partial.
        let partials: Vec<bool> = chart.marks.iter().map(|mark| matches!(mark, Mark::Rect { partial: true, .. })).collect();
        assert_eq!(partials, [false, true]);
        assert_eq!(chart.readouts[1].0, "2025 (partial) · 6.00% of capacity");
        assert_eq!(
            chart.readouts[0].1.last().map(|line| (line.label.as_str(), line.value.as_str())),
            Some(("Refueling", "6.00%")),
            "the bottom of the stack last"
        );
        assert_eq!(table.head[..3], ["Year", "All shown", "Unattributed"]);
        assert_eq!(table.rows[0][..2], ["2024", "6.00%"]);
        assert_eq!(chart.ticks.last().map(|(_, label)| label.as_str()), Some("6%"));
    }

    #[test]
    fn the_scram_chart_reads_rates_per_100_unit_years() {
        let (chart, table) = scram_rate(&[year(2024, false)]);
        // 3,653 unit-days is 10.0 unit-years: 20 automatic and 5 manual are 200 and 50 per 100.
        assert_eq!(chart.readouts[0].0, "2024 · 250.0 per 100 unit-years over 10.0");
        assert_eq!(chart.readouts[0].1.iter().map(|line| line.label.as_str()).collect::<Vec<_>>(), ["Manual (5)", "Automatic (20)"]);
        assert_eq!(table.rows[0], ["2024", "10.0", "20", "5", "200.0", "50.0"]);
        assert_eq!(chart.marks.len(), 2);
    }

    #[test]
    fn the_outage_chart_draws_a_box_and_a_median_per_year() {
        let years = [OutageYear {
            year: 2024,
            count: 3,
            median: 25.0,
            quartiles: (22.5, 32.5),
            shortest: 20,
            longest: 40,
            partial: false,
        }];
        let (chart, table) = outage_length(&years);
        assert_eq!(chart.marks.len(), 2);
        assert!(matches!(&chart.marks[1], Mark::Line { class: "median", .. }));
        assert_eq!(chart.readouts[0].1[1].value, "22.5–32.5 days");
        assert_eq!(table.rows[0], ["2024", "3", "25.0", "22.5", "32.5", "20", "40"]);
    }

    #[test]
    fn the_month_chart_has_twelve_columns_and_counts_the_months_averaged() {
        let mut seasons = [Season::default(); 12];
        seasons[6] = Season {
            month: 7,
            months: 27,
            capacity_kwh: 100,
            lost_kwh: [1, 0, 0, 0, 0, 2, 0, 0],
        };
        let (chart, table) = loss_by_month(&seasons, Families::ALL);
        assert_eq!(chart.columns.len(), 12);
        assert_eq!(chart.readouts[6].0, "Jul, 27 months · 3.00% of capacity");
        assert_eq!(table.rows[6][..2], ["Jul (27)", "3.00%"]);
    }
}
