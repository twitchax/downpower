//! `/trends` (PRD-0006): the fleet over the years in four charts, each a rate so a fleet that
//! changes size doesn't skew it. Reported power reduction as a share of net capacity by year and by
//! calendar month, refueling outage length by year, and Event Notification scrams per 100
//! operating unit-years. The page reads the home page's `ViewState`: the range picks the months,
//! the layer and the families drive the two loss charts, and `table=1` opens every chart's table.
//!
//! Each chart is built natively into a [`Chart`] and a [`Table`] from the data, so what it shows
//! is tested without a browser; [`Plot`] and [`TableView`] only place them.

use std::sync::Arc;

use std::fmt::Write as _;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use downpower_core::series::Monthly;
use downpower_core::trends::{self as fleet, OutageYear, ScramKind, Season, Trends, Year};
use downpower_core::{Families, Family};

use crate::area::{self, HEIGHT, WIDTH};
use crate::bars::{self, Rect};
use crate::filters::Filters;
use crate::state::{ViewState, thin};

/// A column along x: its label, and whether the days shown leave part of it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub label: String,
    pub partial: bool,
}

/// A mark in the viewBox: a rectangle, or a line's path.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Rect { class: String, rect: Rect, partial: bool },
    Line { class: &'static str, path: String, partial: bool },
}

/// One line of a column's readout: its swatch's class, the value as shown, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub swatch: Option<String>,
    pub value: String,
    pub label: String,
}

/// Everything a chart draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub columns: Vec<Column>,
    /// The scale's top, and its ticks with their labels.
    pub top: f64,
    pub ticks: Vec<(f64, String)>,
    pub marks: Vec<Mark>,
    /// Each column's readout: its heading and its lines, top of the stack first.
    pub readouts: Vec<(String, Vec<Line>)>,
}

/// A table view: its caption, its header and its rows, the first cell of each a row label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub caption: String,
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A percentage to two places.
fn percent(value: f64) -> String {
    format!("{value:.2}%")
}

/// A rate or a day count to one place.
fn one(value: f64) -> String {
    format!("{value:.1}")
}

/// A year's label, starred when the days shown leave part of it out.
fn year_label(year: i32, partial: bool) -> String {
    if partial { format!("{year}*") } else { year.to_string() }
}

/// The scale over `peak`, each tick labeled with `unit` after it.
fn scale(peak: f64, unit: &str) -> (f64, Vec<(f64, String)>) {
    let (top, ticks) = area::ticks(peak, 5);
    (top, ticks.into_iter().map(|tick| (tick, format!("{tick}{unit}"))).collect())
}

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

/// The columns whose labels show: all of a dozen months, and past 14 years, every 2nd, 5th or 10th
/// year as the home page's axes thin them.
fn labeled(columns: &[Column]) -> Vec<usize> {
    if columns.len() <= 14 {
        return (0..columns.len()).collect();
    }
    let years: Vec<(i32, f64)> = columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| Some((column.label.trim_end_matches('*').parse().ok()?, f64::from(u32::try_from(index).ok()?))))
        .collect();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "column indexes, held exactly")]
    thin(years, 14).into_iter().map(|(_, index)| index as usize).collect()
}

/// A chart's marks in the stretched viewBox, its scale, its column labels and the hovered column's
/// readout.
#[component]
fn Plot(chart: Memo<Option<Chart>>) -> impl IntoView {
    let pointer = RwSignal::new(None::<f64>);
    let plot = NodeRef::<leptos::html::Div>::new();
    let hovered = Memo::new(move |_| {
        let count = chart.with(|chart| chart.as_ref().map(|chart| chart.columns.len()))?;
        area::month_at(pointer.get()?, count)
    });
    let marks = move || {
        let chart = chart.get()?;
        let count = chart.columns.len();
        let band = hovered.get().map(|index| {
            let (x, width) = bars::column(index, count);
            view! { <rect class="hover" x=format!("{x:.1}") y="0" width=format!("{width:.1}") height=HEIGHT></rect> }
        });
        let gridlines = chart.ticks.iter().fold(String::new(), |mut path, (tick, _)| {
            let _ = write!(path, "M0,{:.2}H{WIDTH}", area::y(*tick, chart.top));
            path
        });
        let marks = chart
            .marks
            .into_iter()
            .map(|mark| match mark {
                Mark::Rect { class, rect, partial } => view! {
                    <rect
                        class=class
                        class:partial=partial
                        x=format!("{:.1}", rect.x)
                        y=format!("{:.2}", rect.y)
                        width=format!("{:.1}", rect.width)
                        height=format!("{:.2}", rect.height)
                    ></rect>
                }
                .into_any(),
                Mark::Line { class, path, partial } => view! { <path class=class class:partial=partial d=path></path> }.into_any(),
            })
            .collect_view();
        Some(view! {
            {band}
            <path class="gridline" d=gridlines></path>
            {marks}
        })
    };
    let y_labels = move || {
        let chart = chart.get()?;
        Some(
            chart
                .ticks
                .into_iter()
                .map(|(tick, label)| view! { <span class="y-tick" style:top=format!("{:.2}%", area::y(tick, chart.top) / HEIGHT * 100.0)>{label}</span> })
                .collect_view(),
        )
    };
    let x_labels = move || {
        let chart = chart.get()?;
        let count = chart.columns.len();
        Some(
            labeled(&chart.columns)
                .into_iter()
                .filter_map(|index| {
                    let column = chart.columns.get(index)?;
                    Some(view! { <span class="tick" style:left=format!("{:.3}%", bars::center(index, count) * 100.0)>{column.label.clone()}</span> })
                })
                .collect_view(),
        )
    };
    let readout = move || {
        let chart = chart.get()?;
        let index = hovered.get()?;
        let (heading, lines) = chart.readouts.get(index)?.clone();
        let fraction = bars::center(index, chart.columns.len());
        let rows = lines
            .into_iter()
            .map(|line| {
                view! {
                    <div class="value">
                        {line.swatch.map(|swatch| view! { <span class=swatch></span> })}
                        <strong>{line.value}</strong>
                        " "
                        {line.label}
                    </div>
                }
            })
            .collect_view();
        let (left, right) = crate::tooltip::beside(&format!("{:.2}%", fraction * 100.0), fraction > 0.7);
        Some(view! {
            <div class="tooltip" role="status" style:left=left style:right=right style:top="8px">
                <div class="meta">{heading}</div>
                {rows}
            </div>
        })
    };

    view! {
        <div
            class="area-plot"
            node_ref=plot
            on:pointermove=move |event| {
                let width = plot.get().map_or(0, |plot| plot.client_width());
                pointer.set((width > 0).then(|| f64::from(event.offset_x()) / f64::from(width)));
            }
            on:pointerleave=move |_| pointer.set(None)
        >
            <svg viewBox=format!("0 0 {WIDTH} {HEIGHT}") preserveAspectRatio="none" role="img" aria-label="A chart of the fleet by year or month; the table view lists every value.">
                {marks}
            </svg>
            <div class="y-axis" aria-hidden="true">{y_labels}</div>
            {readout}
        </div>
        <div class="axis" aria-hidden="true">{x_labels}</div>
    }
}

/// A chart's table view.
#[component]
fn TableView(table: Memo<Option<Table>>) -> impl IntoView {
    move || {
        let table = table.get()?;
        let head = table.head.into_iter().map(|cell| view! { <th scope="col">{cell}</th> }).collect_view();
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
        Some(view! {
            <div class="table-scroll">
                <table class="totals">
                    <caption class="meta">{table.caption}</caption>
                    <thead>
                        <tr>{head}</tr>
                    </thead>
                    <tbody>{rows}</tbody>
                </table>
            </div>
        })
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
    fn past_fourteen_years_the_labels_thin_and_months_all_show() {
        let columns = |labels: Vec<String>| labels.into_iter().map(|label| Column { label, partial: false }).collect::<Vec<_>>();
        let years = columns((1999..=2026).map(|year| year.to_string()).collect());
        assert_eq!(labeled(&years), [1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27], "every even year");
        assert_eq!(labeled(&columns(MONTHS.map(str::to_owned).to_vec())).len(), 12);
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
