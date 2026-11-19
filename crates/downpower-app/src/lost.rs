//! The stacked area of reported power reduction by family over time, the barcode's summary
//! (DESIGN.md Decisions 22 and 24). It shares the barcode's filter state: the layer picks the series,
//! the range picks the months, and a hidden family drops out of the stack. A table view carries
//! every number the chart shows, and rides in the URL as `table=1` like every other control.

use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{Datelike as _, NaiveDate};
use leptos::prelude::*;

use downpower_core::Family;
use downpower_core::barcode::Layer;
use downpower_core::series::Monthly;

use crate::area::{self, HEIGHT, WIDTH};
use crate::state::{ViewState, thin};

/// Kilowatt-hours in a terawatt-hour.
const TWH: f64 = 1e9;

/// The months in view, in terawatt-hours.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    first: NaiveDate,
    months: Vec<[f64; 8]>,
}

impl Shown {
    fn new(monthly: &Monthly, view: &ViewState) -> Option<Self> {
        let range = monthly.range(view.from, view.to);
        let first = monthly.month(range.start)?;
        #[allow(clippy::cast_precision_loss, reason = "kWh totals far below 2^52")]
        let months = monthly.lost_kwh.get(range)?.iter().map(|month| month.map(|kwh| kwh as f64 / TWH)).collect();
        Some(Self { first, months })
    }

    fn month(&self, index: usize) -> NaiveDate {
        self.first.checked_add_months(chrono::Months::new(u32::try_from(index).unwrap_or(0))).unwrap_or(self.first)
    }

    /// Each family's total over the months shown.
    fn totals(&self) -> [f64; 8] {
        self.months.iter().fold([0.0; 8], |mut totals, month| {
            for (total, value) in totals.iter_mut().zip(month) {
                *total += value;
            }
            totals
        })
    }

    /// January marks, at their fraction of the width.
    fn years(&self, most: usize) -> Vec<(i32, f64)> {
        let count = self.months.len();
        let years = (0..count)
            .filter(|index| self.month(*index).month() == 1)
            .map(|index| (self.month(index).year(), area::x(index, count) / WIDTH))
            .collect();
        thin(years, most)
    }
}

/// Terawatt-hours to one decimal, or to three under a tenth.
fn twh(value: f64) -> String {
    if value < 0.1 { format!("{value:.3}") } else { format!("{value:.1}") }
}

#[cfg(feature = "hydrate")]
pub(crate) async fn load(layer: Layer) -> Result<Arc<Monthly>, String> {
    let bytes = crate::browser::fetch(&format!("/api/lost/{}", layer.slug())).await?.ok_or("no series yet")?;
    serde_json::from_slice(&bytes).map(Arc::new).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
pub(crate) async fn load(_layer: Layer) -> Result<Arc<Monthly>, String> {
    Err("the series loads in the browser".to_owned())
}

#[component]
pub fn StackedArea(state: Memo<ViewState>) -> impl IntoView {
    let layer = Memo::new(move |_| state.get().layer);
    let data = LocalResource::new(move || load(layer.get()));
    let shown = Memo::new(move |_| {
        let monthly = data.get()?.ok()?;
        Shown::new(&monthly, &state.get())
    });
    let pointer = RwSignal::new(None::<f64>);
    let table = Memo::new(move |_| state.get().table);
    let go = crate::filters::navigator();
    let plot = NodeRef::<leptos::html::Div>::new();

    let scale = Memo::new(move |_| {
        let shown = shown.get()?;
        Some(area::ticks(area::peak(&shown.months, state.get().families), 5))
    });
    let bands = move || {
        let shown = shown.get()?;
        let (top, _) = scale.get()?;
        let bands = area::bands(&area::stack(&shown.months, state.get().families), top);
        let fills = bands
            .iter()
            .map(|band| view! { <path class=format!("band family-{}", band.key.slug()) d=band.path.clone()></path> })
            .collect_view();
        let seams = area::seamed(shown.months.len()).then(|| bands.into_iter().map(|band| view! { <path class="seam" d=band.top></path> }).collect_view());
        Some(view! { {fills} {seams} })
    };
    let gridlines = move || {
        let (top, ticks) = scale.get()?;
        let path = ticks.iter().fold(String::new(), |mut path, tick| {
            let _ = write!(path, "M0,{:.2}H{WIDTH}", area::y(*tick, top));
            path
        });
        Some(view! { <path class="gridline" d=path></path> })
    };
    let y_labels = move || {
        let (top, ticks) = scale.get()?;
        Some(
            ticks
                .into_iter()
                .map(|tick| view! { <span class="y-tick" style:top=format!("{:.2}%", area::y(tick, top) / HEIGHT * 100.0)>{format!("{tick}")}</span> })
                .collect_view(),
        )
    };
    let x_labels = move || {
        Some(
            shown
                .get()?
                .years(14)
                .into_iter()
                .map(|(year, fraction)| view! { <span class="tick" class:end=crate::state::hangs_left(fraction) class:minor=crate::state::minor(year) style:left=format!("{:.3}%", fraction * 100.0)>{year}</span> })
                .collect_view(),
        )
    };
    let crosshair = move || {
        let shown = shown.get()?;
        let index = area::month_at(pointer.get()?, shown.months.len())?;
        let x = f64::midpoint(area::x(index, shown.months.len()), area::x(index + 1, shown.months.len()));
        Some(view! { <path class="crosshair" d=format!("M{x:.1},0V{HEIGHT}")></path> })
    };
    let status = move || match data.get() {
        None => Some("loading…".to_owned()),
        Some(Err(error)) => Some(error),
        Some(Ok(_)) => None,
    };

    view! {
        <figure class="area">
            <figcaption class="row">
                <span class="label">"Reported power reduction"</span>
                <span class="meta">"TWh per month, by cause family"</span>
                <button class="btn" aria-pressed=move || table.get().to_string() on:click=move |_| go(ViewState { table: !table.get_untracked(), ..state.get_untracked() })>
                    "Table"
                </button>
            </figcaption>
            <div
                class="area-plot"
                node_ref=plot
                on:pointermove=move |event| {
                    let width = plot.get().map_or(0, |plot| plot.client_width());
                    pointer.set((width > 0).then(|| f64::from(event.offset_x()) / f64::from(width)));
                }
                on:pointerleave=move |_| pointer.set(None)
            >
                <svg viewBox=format!("0 0 {WIDTH} {HEIGHT}") preserveAspectRatio="none" role="img" aria-label="Stacked area of reported power reduction per month by cause family; the table view lists the totals.">
                    {gridlines}
                    {bands}
                    {crosshair}
                </svg>
                <div class="y-axis" aria-hidden="true">{y_labels}</div>
                <Readout shown pointer state />
                {move || status().map(|status| view! { <p class="plot-status meta">{status}</p> })}
            </div>
            <div class="axis" aria-hidden="true">{x_labels}</div>
            {move || table.get().then(|| view! { <Totals shown state /> })}
        </figure>
    }
}

/// The crosshair's readout: every visible family's value that month, top of the stack first.
#[component]
fn Readout(shown: Memo<Option<Shown>>, pointer: RwSignal<Option<f64>>, state: Memo<ViewState>) -> impl IntoView {
    move || {
        let shown = shown.get()?;
        let fraction = pointer.get()?;
        let index = area::month_at(fraction, shown.months.len())?;
        let month = shown.months.get(index)?;
        let visible = state.get().families;
        let total: f64 = visible.iter().map(|family| month[usize::from(family.index())]).sum();
        let rows = Family::STACK
            .into_iter()
            .rev()
            .filter(|family| visible.contains(*family))
            .map(|family| {
                view! {
                    <div class="value">
                        <span class=format!("key family-{}", family.slug())></span>
                        <strong>{twh(month[usize::from(family.index())])}</strong>
                        " "
                        {family.label()}
                    </div>
                }
            })
            .collect_view();
        let (left, right) = crate::tooltip::beside(&format!("{:.2}%", fraction * 100.0), fraction > 0.7);
        Some(view! {
            <div class="tooltip" role="status" style:left=left style:right=right style:top="8px">
                <div class="meta">{shown.month(index).format("%B %Y").to_string()} " · " {twh(total)} " TWh in all"</div>
                {rows}
            </div>
        })
    }
}

/// The table view: each family's reported power reduction over the range, and its share.
#[component]
fn Totals(shown: Memo<Option<Shown>>, state: Memo<ViewState>) -> impl IntoView {
    move || {
        let shown = shown.get()?;
        let totals = shown.totals();
        let visible = state.get().families;
        let sum: f64 = totals.iter().sum();
        let last = shown.month(shown.months.len().saturating_sub(1));
        let rows = Family::STACK
            .into_iter()
            .map(|family| {
                let value = totals[usize::from(family.index())];
                let share = if sum > 0.0 { value / sum * 100.0 } else { 0.0 };
                view! {
                    <tr class:hidden=!visible.contains(family)>
                        <th scope="row">
                            <span class="row-label">
                                <span class=format!("swatch family-{}", family.slug())></span>
                                {family.label()}
                            </span>
                        </th>
                        <td>{twh(value)}</td>
                        <td>{format!("{share:.1}%")}</td>
                    </tr>
                }
            })
            .collect_view();
        Some(view! {
            <table class="totals">
                <caption class="meta">{format!("{} to {}, every family, TWh", shown.first.format("%B %Y"), last.format("%B %Y"))}</caption>
                <thead>
                    <tr>
                        <th scope="col">"Family"</th>
                        <th scope="col">"TWh"</th>
                        <th scope="col">"Share"</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
                <tfoot>
                    <tr>
                        <th scope="row">"All"</th>
                        <td>{twh(sum)}</td>
                        <td>"100%"</td>
                    </tr>
                </tfoot>
            </table>
        })
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap_or_default()
    }

    fn monthly() -> anyhow::Result<Monthly> {
        let mut monthly = Monthly::default();
        monthly.add(date("2023-11-02"), Family::Refueling, 2_000_000_000)?;
        monthly.add(date("2024-01-02"), Family::Forced, 500_000_000)?;
        monthly.add(date("2024-02-02"), Family::Refueling, 1_000_000_000)?;
        Ok(monthly)
    }

    #[test]
    fn the_range_picks_the_months_and_totals_add_up() -> anyhow::Result<()> {
        let all = Shown::new(&monthly()?, &ViewState::default()).ok_or_else(|| anyhow::anyhow!("months"))?;
        assert_eq!(all.first, date("2023-11-01"));
        assert_eq!(all.months.len(), 4);
        let totals = all.totals();
        assert!((totals[usize::from(Family::Refueling.index())] - 3.0).abs() < 1e-9);
        assert_eq!(all.years(10).iter().map(|(year, _)| *year).collect::<Vec<_>>(), [2024]);

        let view = ViewState {
            from: Some(date("2024-01-15")),
            ..ViewState::default()
        };
        let ranged = Shown::new(&monthly()?, &view).ok_or_else(|| anyhow::anyhow!("months"))?;
        assert_eq!((ranged.first, ranged.months.len()), (date("2024-01-01"), 2));
        Ok(())
    }

    #[test]
    fn small_values_keep_their_digits() {
        assert_eq!(twh(12.345), "12.3");
        assert_eq!(twh(0.0123), "0.012");
    }
}
