//! The fleet barcode, the hero view (DESIGN.md Decisions 23 and 24): a row per unit, time running
//! left to right, each reduced span in its family's color. It draws on a canvas from the packed
//! buffer `/api/barcode/{layer}` serves, and hovering a cell fetches that unit-day's row.

use std::sync::Arc;

use chrono::NaiveDate;
use leptos::prelude::*;
use serde::Deserialize;

use downpower_core::Family;
use downpower_core::barcode::{Barcode, Cell, Layer};

use crate::paint::{self, Window};
use crate::state::{ViewState, year_ticks};

/// CSS pixels per unit row, a pixel of which is the gap between rows.
pub const ROW: f64 = 4.0;

/// One unit-day as `/api/unit-day` serves it: the fields the tooltip shows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Detail {
    pub docket: String,
    pub date: NaiveDate,
    pub power: u8,
    pub reason: Option<String>,
    pub cause: Option<String>,
    pub family: Family,
    pub method: String,
}

/// What the pointer (or the arrow keys) points at, in CSS pixels within the plot.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pointer {
    x: f64,
    y: f64,
}

/// The cell under the pointer.
#[derive(Debug, Clone, PartialEq)]
struct Target {
    docket: String,
    name: String,
    date: NaiveDate,
    cell: Cell,
    /// Days the pixel column spans, more than one when zoomed out.
    span: u32,
    pointer: Pointer,
}

/// A method's name, said in words.
fn method(method: &str) -> &'static str {
    match method {
        "precedence_v1" => "named on the day",
        "inherit_14d" => "inherited from an earlier day",
        "xref_unresolved" => "an event report, not yet resolved",
        "no_cause" => "no cause given",
        "reason_pending" => "reason not yet published",
        "no_report" => "no NRC report; EIA's number",
        "unknown_tag" => "an unrecognized reason",
        _ => "",
    }
}

/// Where a row's label comes from: its cause, if any, and the method in words.
fn provenance(row: &Detail) -> String {
    [row.cause.as_ref().map(|cause| cause.replace('_', " ")), Some(method(&row.method).to_owned())]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A cell's power, as precise as the bucket allows.
fn power(cell: Cell) -> String {
    let range = cell.power();
    if range.start() == range.end() {
        format!("{}%", range.start())
    } else {
        format!("{}–{}%", range.start(), range.end())
    }
}

#[cfg(feature = "hydrate")]
async fn load(layer: Layer) -> Result<Arc<Barcode>, String> {
    let bytes = crate::browser::fetch(&format!("/api/barcode/{}", layer.slug())).await?.ok_or("no barcode yet")?;
    Barcode::decode(&bytes).map(Arc::new).map_err(|error| format!("{error:#}"))
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load(_layer: Layer) -> Result<Arc<Barcode>, String> {
    Err("the barcode loads in the browser".to_owned())
}

/// The decoded buffer, once loaded.
type Loaded = Signal<Option<Arc<Barcode>>>;

#[component]
pub fn BarcodeView(state: Memo<ViewState>) -> impl IntoView {
    let layer = Memo::new(move |_| state.get().layer);
    let data = LocalResource::new(move || load(layer.get()));
    let barcode: Loaded = Signal::derive(move || data.get().and_then(Result::ok));
    let canvas = NodeRef::<leptos::html::Canvas>::new();
    let swatches = NodeRef::<leptos::html::Div>::new();
    let pointer = RwSignal::new(None::<Pointer>);
    let detail = RwSignal::new(None::<Detail>);
    // The plot's CSS width, which the draw keeps; a resize redraws.
    let width = RwSignal::new(0_u32);
    let resized = RwSignal::new(0_u32);
    // In an effect, which only the browser runs: the listener needs a window, and a server whose
    // build also enables `hydrate` (a whole-workspace test build) would panic rendering it.
    Effect::new(move |_| {
        let handle = window_event_listener(leptos::ev::resize, move |_| resized.update(|count| *count += 1));
        on_cleanup(move || handle.remove());
    });

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        let view = state.get();
        resized.track();
        let (Some(barcode), Some(canvas), Some(swatches)) = (barcode.get(), canvas.get(), swatches.get()) else {
            return;
        };
        let Some(palette) = crate::browser::palette(&swatches) else {
            leptos::logging::error!("the barcode's swatches have no computed colors");
            return;
        };
        width.set(u32::try_from(canvas.client_width()).unwrap_or(0));
        let window = Window::new(&barcode, view.from, view.to);
        if let Err(error) = crate::browser::draw(&canvas, &barcode, window, view.families, &palette, ROW) {
            leptos::logging::error!("drawing the barcode: {error}");
        }
    });

    let target = Memo::new(move |_| {
        let loaded = barcode.get()?;
        aim(pointer.get()?, &loaded, &state.get(), width.get())
    });
    #[cfg(feature = "hydrate")]
    fetch_detail(target, detail);

    let rows = move || barcode.get().map_or(0, |barcode| u32::try_from(barcode.units().len()).unwrap_or(0));
    let status = move || match data.get() {
        None => Some("loading the ledger…".to_owned()),
        Some(Err(error)) => Some(error),
        Some(Ok(_)) => None,
    };

    view! {
        <figure class="barcode">
            <div class="plot">
                <canvas
                    node_ref=canvas
                    tabindex="0"
                    style:height=move || format!("{}px", f64::from(rows()) * ROW)
                    aria-label="The fleet barcode: one row per reactor unit, time running left to right, each reduced day colored by its cause family. The legend above filters it; the arrow keys move a cursor over it."
                    on:pointermove=move |event| pointer.set(Some(Pointer { x: f64::from(event.offset_x()), y: f64::from(event.offset_y()) }))
                    on:pointerleave=move |_| pointer.set(None)
                    on:blur=move |_| pointer.set(None)
                    on:keydown=move |event| {
                        let key = event.key();
                        if key == "Escape" {
                            pointer.set(None);
                        } else if let Some(step) = step(&key) {
                            event.prevent_default();
                            let bounds = (f64::from(width.get_untracked().saturating_sub(1)), (f64::from(rows()) * ROW - 1.0).max(0.0));
                            pointer.update(|pointer| *pointer = Some(moved(*pointer, step, bounds)));
                        }
                    }
                ></canvas>
                <Tooltip target detail width />
                {move || status().map(|status| view! { <p class="plot-status meta">{status}</p> })}
            </div>
            <Axis barcode state />
            // The colors the canvas paints with, read off the stylesheet at each draw.
            <div class="swatches" node_ref=swatches aria-hidden="true">
                {Family::ALL.map(|family| view! { <span data-paint=family.slug() class=format!("family-{}", family.slug())></span> }).collect_view()}
                <span data-paint="track"></span>
                <span data-paint="hidden"></span>
            </div>
        </figure>
    }
}

/// The cell `pointer` lands on in a plot `width` CSS pixels wide.
fn aim(pointer: Pointer, barcode: &Barcode, view: &ViewState, width: u32) -> Option<Target> {
    let window = Window::new(barcode, view.from, view.to);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a pixel inside the plot")]
    let (row, x) = ((pointer.y / ROW).floor().max(0.0) as usize, pointer.x.floor().max(0.0) as u32);
    let unit = barcode.units().get(row)?;
    let days = window.span(x.min(width.saturating_sub(1)), width.max(1));
    let index = paint::deepest(barcode, row, days.clone())?;
    Some(Target {
        docket: unit.docket.clone(),
        name: unit.name.clone(),
        date: barcode.date(index),
        cell: barcode.cell(row, index)?,
        span: days.end - days.start,
        pointer,
    })
}

/// How an arrow key moves the keyboard cursor: a pixel column across, a row up or down.
fn step(key: &str) -> Option<(f64, f64)> {
    match key {
        "ArrowLeft" => Some((-1.0, 0.0)),
        "ArrowRight" => Some((1.0, 0.0)),
        "ArrowUp" => Some((0.0, -ROW)),
        "ArrowDown" => Some((0.0, ROW)),
        _ => None,
    }
}

/// The cursor after `step`, kept inside `bounds`. With no cursor yet it starts at the newest day of
/// the first row.
fn moved(pointer: Option<Pointer>, step: (f64, f64), bounds: (f64, f64)) -> Pointer {
    let at = pointer.unwrap_or(Pointer { x: bounds.0, y: ROW / 2.0 });
    Pointer {
        x: (at.x + step.0).clamp(0.0, bounds.0),
        y: (at.y + step.1).clamp(0.0, bounds.1),
    }
}

/// Fetches the row behind the hovered cell once the pointer rests on it.
#[cfg(feature = "hydrate")]
fn fetch_detail(target: Memo<Option<Target>>, detail: RwSignal<Option<Detail>>) {
    Effect::new(move |_| {
        let Some(Target { docket, date, .. }) = target.get() else { return };
        if detail.with_untracked(|detail| detail.as_ref().is_some_and(|detail| detail.docket == docket && detail.date == date)) {
            return;
        }
        set_timeout(
            move || {
                let still = target.with_untracked(|target| target.as_ref().is_some_and(|target| target.docket == docket && target.date == date));
                if !still {
                    return;
                }
                leptos::task::spawn_local(async move {
                    let url = format!("/api/unit-day/{docket}/{date}");
                    match crate::browser::fetch(&url).await.map(|bytes| bytes.map(|bytes| serde_json::from_slice::<Detail>(&bytes))) {
                        Ok(Some(Ok(row))) => detail.set(Some(row)),
                        Ok(Some(Err(error))) => leptos::logging::error!("{url}: {error}"),
                        Ok(None) => {}
                        Err(error) => leptos::logging::error!("{error}"),
                    }
                });
            },
            std::time::Duration::from_millis(120),
        );
    });
}

/// The hover readout: the value first, then the unit and day, then the reason once its row loads.
#[component]
fn Tooltip(target: Memo<Option<Target>>, detail: RwSignal<Option<Detail>>, width: RwSignal<u32>) -> impl IntoView {
    move || {
        let target = target.get()?;
        let row = detail.get().filter(|detail| detail.docket == target.docket && detail.date == target.date);
        let family = row.as_ref().map_or(target.cell.family, |row| row.family);
        let value = row.as_ref().map_or_else(|| power(target.cell), |row| format!("{}%", row.power));
        // A family names why power is down, so a day at full power gets none.
        let full = row.as_ref().map_or(target.cell.bucket == downpower_core::barcode::FULL, |row| row.power >= 100);
        let label = if full { "full power" } else { family.label() };
        // Flip to the pointer's left near the plot's right edge.
        let left = if f64::from(width.get()) - target.pointer.x < 260.0 {
            format!("calc({:.0}px - 100% - 12px)", target.pointer.x)
        } else {
            format!("{:.0}px", target.pointer.x + 12.0)
        };
        Some(view! {
            <div class="tooltip" role="status" style:left=left style:top=format!("{:.0}px", target.pointer.y + 12.0)>
                <div class="value">
                    {(!full).then(|| view! { <span class=format!("key family-{}", family.slug())></span> })}
                    <strong>{value}</strong>
                    " "
                    {label}
                </div>
                <div class="meta">
                    {target.name.clone()} " · " {target.date.to_string()}
                    {(target.span > 1).then(|| format!(" · 1 of {} days in this pixel", target.span))}
                </div>
                {row.map(|row| view! {
                    <div class="reason">{row.reason.clone().unwrap_or_else(|| "(no reason text)".to_owned())}</div>
                    <div class="meta">{provenance(&row)}</div>
                })}
            </div>
        })
    }
}

/// Year marks under the plot.
#[component]
fn Axis(barcode: Loaded, state: Memo<ViewState>) -> impl IntoView {
    let ticks = move || {
        let barcode = barcode.get()?;
        let view = state.get();
        let window = Window::new(&barcode, view.from, view.to);
        Some(
            year_ticks(barcode.date(window.start), window.days, 14)
                .into_iter()
                .map(|(year, fraction)| view! { <span class="tick" style:left=format!("{:.3}%", fraction * 100.0)>{year}</span> })
                .collect_view(),
        )
    };
    view! { <div class="axis" aria-hidden="true">{ticks}</div> }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn powers_read_as_precisely_as_the_bucket_allows() {
        assert_eq!(power(Cell::new(Family::Forced, 100)), "100%");
        assert_eq!(power(Cell::new(Family::Forced, 50)), "50–53%");
    }

    #[test]
    fn the_keyboard_cursor_starts_at_the_newest_day_and_stays_inside() {
        let bounds = (99.0, 39.0);
        let first = moved(None, (1.0, 0.0), bounds);
        assert_eq!(first, Pointer { x: 99.0, y: ROW / 2.0 });
        assert_eq!(moved(Some(first), (0.0, -ROW), bounds), Pointer { x: 99.0, y: 0.0 });
        assert_eq!(step("ArrowDown"), Some((0.0, ROW)));
        assert_eq!(step("a"), None);
    }

    #[test]
    fn provenance_joins_only_what_is_there() {
        let row = |cause: Option<&str>, method: &str| Detail {
            docket: "05000323".to_owned(),
            date: NaiveDate::default(),
            power: 50,
            reason: None,
            cause: cause.map(str::to_owned),
            family: Family::Forced,
            method: method.to_owned(),
        };
        assert_eq!(provenance(&row(Some("reactor_trip"), "precedence_v1")), "reactor trip · named on the day");
        assert_eq!(provenance(&row(None, "no_cause")), "no cause given");
    }

    #[test]
    fn every_method_reads_in_words() {
        for name in ["precedence_v1", "inherit_14d", "xref_unresolved", "no_cause", "reason_pending", "no_report", "unknown_tag"] {
            assert!(!method(name).is_empty(), "{name}");
        }
    }
}
