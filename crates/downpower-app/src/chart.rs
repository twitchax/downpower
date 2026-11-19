//! What the trends and FFD pages draw with (PRD-0006 T-004, PRD-0005 T-007): a chart built
//! natively into a [`Chart`] and a [`Table`] from the data, so what it shows is tested without a
//! browser, and the two components that only place them, [`Plot`] and [`TableView`]. Bars and
//! spreads sit in the stacked area's stretched viewBox ([`bars`]), every label in HTML beside it.

use std::fmt::Write as _;

use leptos::prelude::*;

use crate::area::{self, HEIGHT, WIDTH};
use crate::bars::{self, Rect};
use crate::state::thin;

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
pub(crate) fn percent(value: f64) -> String {
    format!("{value:.2}%")
}

/// A rate or a day count to one place.
pub(crate) fn one(value: f64) -> String {
    format!("{value:.1}")
}

/// A year's label, starred when the days shown leave part of it out.
pub(crate) fn year_label(year: i32, partial: bool) -> String {
    if partial { format!("{year}*") } else { year.to_string() }
}

/// The scale over `peak`, each tick labeled with `unit` after it.
pub(crate) fn scale(peak: f64, unit: &str) -> (f64, Vec<(f64, String)>) {
    let (top, ticks) = area::ticks(peak, 5);
    (top, ticks.into_iter().map(|tick| (tick, format!("{tick}{unit}"))).collect())
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
pub(crate) fn Plot(chart: Memo<Option<Chart>>) -> impl IntoView {
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
                    let fraction = bars::center(index, count);
                    Some(view! { <span
                    class="tick"
                    class:end=crate::state::hangs_left(fraction)
                    class:minor=column.label.trim_end_matches('*').parse().is_ok_and(crate::state::minor)
                    style:left=format!("{:.3}%", fraction * 100.0)>{column.label.clone()}</span> })
                })
                .collect_view(),
        )
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
            <Readout chart hovered />
        </div>
        <div class="axis" aria-hidden="true">{x_labels}</div>
    }
}

/// The hovered column's readout, beside the pointer.
#[component]
fn Readout(chart: Memo<Option<Chart>>, hovered: Memo<Option<usize>>) -> impl IntoView {
    move || {
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
    }
}

/// A chart's table view.
#[component]
pub(crate) fn TableView(table: Memo<Option<Table>>) -> impl IntoView {
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

    #[test]
    fn past_fourteen_years_the_labels_thin_and_months_all_show() {
        let columns = |labels: Vec<String>| labels.into_iter().map(|label| Column { label, partial: false }).collect::<Vec<_>>();
        let years = columns((1999..=2026).map(|year| year.to_string()).collect());
        assert_eq!(labeled(&years), [1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27], "every even year");
        assert_eq!(
            labeled(&columns(
                ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].map(str::to_owned).to_vec()
            ))
            .len(),
            12
        );
    }
}
