//! `/events`: every Event Notification since 1999, filtered from one row of controls kept in the
//! URL (PRD-0004 T-004, DESIGN.md Decision 34). The counts chart stacks the selected events per
//! month in eight series, the seven largest category groups and Other, on the stacked area's
//! geometry; the list below pages through them newest first. Both load in the browser.

use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{Datelike as _, Months, NaiveDate};
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_query_map};

use downpower_core::explorer::{Class, EventCounts, EventRow, EventsPage, EventsState, Group, SERIES};
use downpower_core::triggers::System;

use crate::area::{self, HEIGHT, WIDTH};
use crate::state::thin;

/// The label of series `index`: a group, or Other for the groups past the seventh.
#[must_use]
pub fn series_label(index: usize) -> &'static str {
    match Group::ALL.get(index) {
        Some(group) if index < SERIES - 1 => group.label(),
        _ => "Other",
    }
}

/// The counts in view, as the area draws them.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    first: NaiveDate,
    months: Vec<[f64; SERIES]>,
}

impl Shown {
    fn new(counts: &EventCounts) -> Option<Self> {
        let first = counts.first?;
        #[allow(clippy::cast_precision_loss, reason = "event counts far below 2^52")]
        let months = counts.months.iter().map(|month| month.map(|count| count as f64)).collect();
        Some(Self { first, months })
    }

    fn month(&self, index: usize) -> NaiveDate {
        self.first.checked_add_months(Months::new(u32::try_from(index).unwrap_or(0))).unwrap_or(self.first)
    }

    fn years(&self, most: usize) -> Vec<(i32, f64)> {
        let count = self.months.len();
        let years = (0..count)
            .filter(|index| self.month(*index).month() == 1)
            .map(|index| (self.month(index).year(), area::x(index, count) / WIDTH))
            .collect();
        thin(years, most)
    }
}

/// The pages either side of `page`: newer, then older.
#[must_use]
pub const fn neighbors(page: &EventsPage) -> (Option<usize>, Option<usize>) {
    let newer = if page.page > 1 { Some(page.page - 1) } else { None };
    let older = if page.page < page.pages { Some(page.page + 1) } else { None };
    (newer, older)
}

/// What the list says above its rows.
#[must_use]
pub fn summary(page: &EventsPage) -> String {
    match page.total {
        0 => "No events match.".to_owned(),
        1 => "1 event.".to_owned(),
        total => format!("{total} events, newest first; page {} of {}.", page.page, page.pages),
    }
}

/// Writes a state to the URL, replacing the current history entry.
fn navigator() -> impl Fn(EventsState) + Clone {
    let navigate = use_navigate();
    move |next: EventsState| {
        navigate(
            &format!("/events{}", next.to_query()),
            NavigateOptions {
                replace: true,
                scroll: false,
                ..NavigateOptions::default()
            },
        );
    }
}

#[cfg(feature = "hydrate")]
async fn load<T: serde::de::DeserializeOwned>(url: String) -> Result<Arc<T>, String> {
    let bytes = crate::browser::fetch(&url).await?.ok_or("nothing there")?;
    serde_json::from_slice(&bytes).map(Arc::new).map_err(|error| error.to_string())
}

#[cfg(not(feature = "hydrate"))]
#[allow(clippy::unused_async, reason = "the browser's version awaits")]
async fn load<T>(_url: String) -> Result<Arc<T>, String> {
    Err("the events load in the browser".to_owned())
}

#[component]
pub fn EventsExplorer() -> impl IntoView {
    let query = use_query_map();
    let state = Memo::new(move |_| query.with(|query| EventsState::from_query(|name| query.get(name))));
    view! {
        <header class="intro">
            <h1>"Events"</h1>
            <p class="muted">"Every Event Notification the NRC has published since 1999: reactors, materials licensees, fuel facilities and vendors."</p>
        </header>
        <ExplorerFilters state />
        <CountsChart state />
        <EventList state />
    }
}

/// The filter row: dates, place and code, emergency class, scram and its trigger, fitness for duty, words; then the category groups,
/// which double as the chart's legend.
#[component]
fn ExplorerFilters(state: Memo<EventsState>) -> impl IntoView {
    let go = navigator();
    let text = |label: &'static str, placeholder: &'static str, width: &'static str, get: fn(&EventsState) -> Option<String>, set: fn(&mut EventsState, Option<String>)| {
        let go = go.clone();
        view! {
            <label class="date">
                <span class="label">{label}</span>
                <input
                    type="search"
                    placeholder=placeholder
                    style:width=width
                    prop:value=move || get(&state.get()).unwrap_or_default()
                    on:change=move |event| {
                        let mut next = state.get_untracked();
                        let value = event_target_value(&event);
                        set(&mut next, (!value.trim().is_empty()).then_some(value));
                        go(EventsState::parse_query(&next.to_query()).first_page());
                    }
                />
            </label>
        }
    };
    let date = |label: &'static str, get: fn(&EventsState) -> Option<NaiveDate>, set: fn(&mut EventsState, Option<NaiveDate>)| {
        let go = go.clone();
        view! {
            <label class="date">
                <span class="label">{label}</span>
                <input
                    type="date"
                    prop:value=move || get(&state.get()).map(|date| date.to_string()).unwrap_or_default()
                    on:change=move |event| {
                        let mut next = state.get_untracked();
                        set(&mut next, event_target_value(&event).parse().ok());
                        go(next.first_page());
                    }
                />
            </label>
        }
    };
    let words = text("search", "words in the text", "22ch", |state| state.q.clone(), |state, value| state.q = value);
    let place = text("place", "facility or licensee", "22ch", |state| state.facility.clone(), |state, value| state.facility = value);
    let us_state = text("state", "TX", "6ch", |state| state.state.clone(), |state, value| state.state = value);
    let cfr = text("10 CFR", "50.72(b)(2)", "16ch", |state| state.cfr.clone(), |state, value| state.cfr = value);
    let from = date("from", |state| state.from, |state, date| state.from = date);
    let to = date("to", |state| state.to, |state, date| state.to = date);
    let classes = Class::ALL
        .map(|class| view! { <option value=class.slug() selected=move || state.get().class == Some(class)>{class.label()}</option> })
        .collect_view();
    let class = {
        let go = go.clone();
        view! {
            <label class="date">
                <span class="label">"class"</span>
                <select on:change=move |event| go(EventsState { class: Class::from_slug(&event_target_value(&event)), ..state.get_untracked() }.first_page())>
                    <option value="" selected=move || state.get().class.is_none()>"any"</option>
                    {classes}
                </select>
            </label>
        }
    };
    let systems = System::ALL
        .map(|system| view! { <option value=system.slug() selected=move || state.get().triggers == [system]>{system.label()}</option> })
        .collect_view();
    let trigger = {
        let go = go.clone();
        view! {
            <label class="date" title="Only scrams whose trigger, as the phrase rules read the text, is this system">
                <span class="label">"trigger"</span>
                <select on:change=move |event| {
                    let triggers = System::from_slug(&event_target_value(&event)).into_iter().collect();
                    go(EventsState { triggers, ..state.get_untracked() }.first_page());
                }>
                    <option value="" selected=move || state.get().triggers.is_empty()>"any"</option>
                    {systems}
                </select>
            </label>
        }
    };
    let scram = {
        let go = go.clone();
        view! {
            <button
                class="btn"
                aria-pressed=move || state.get().scram.to_string()
                title="Only events whose unit table gives a reactor a SCRAM Code other than N"
                on:click=move |_| {
                    let current = state.get_untracked();
                    go(EventsState { scram: !current.scram, ..current }.first_page());
                }
            >
                "Scrams only"
            </button>
        }
    };
    let ffd = {
        let go = go.clone();
        view! {
            <button
                class="btn"
                aria-pressed=move || state.get().ffd.to_string()
                title="Only fitness-for-duty events: those coded 10 CFR 26.73, 26.719 or 26.417(b)(1), plus a reviewed list"
                on:click=move |_| {
                    let current = state.get_untracked();
                    go(EventsState { ffd: !current.ffd, ..current }.first_page());
                }
            >
                "Fitness for duty"
            </button>
        }
    };
    let unit = {
        let go = go.clone();
        move || {
            let go = go.clone();
            state.get().unit.map(|unit| {
                view! {
                    <button class="btn" title="Show every unit" on:click=move |_| go(EventsState { unit: None, ..state.get_untracked() }.first_page())>
                        {format!("unit {unit} ×")}
                    </button>
                }
            })
        }
    };
    let clear = {
        let go = go.clone();
        move || {
            let go = go.clone();
            (state.get() != EventsState::default()).then(|| view! { <button class="btn" on:click=move |_| go(EventsState::default())>"Clear"</button> })
        }
    };
    view! {
        <div class="filters">
            <div class="row" role="group" aria-label="Range">{from} {to}</div>
            {words}
            {place}
            {us_state}
            {cfr}
            {class}
            {scram}
            {trigger}
            {ffd}
            {unit}
            {clear}
        </div>
        <Groups state />
    }
}

/// The category groups, each a toggle. With none chosen every group is listed, so every toggle reads
/// pressed; pressing one then lists it alone.
#[component]
fn Groups(state: Memo<EventsState>) -> impl IntoView {
    let go = navigator();
    let items = Group::ALL
        .map(|group| {
            let go = go.clone();
            view! {
                <button
                    class="legend-item"
                    aria-pressed=move || {
                        let groups = state.get().groups;
                        (groups.is_empty() || groups.contains(&group)).to_string()
                    }
                    on:click=move |_| {
                        let mut next = state.get_untracked();
                        if let Some(at) = next.groups.iter().position(|selected| *selected == group) {
                            next.groups.remove(at);
                        } else {
                            next.groups.push(group);
                            next.groups.sort();
                        }
                        go(next.first_page());
                    }
                >
                    <span class=format!("swatch series-{}", group.series())></span>
                    {group.label()}
                </button>
            }
        })
        .collect_view();
    view! { <div class="legend" role="group" aria-label="Categories: press some to list only those">{items}</div> }
}

/// Events per month by category, stacked: the selected events, all months they span.
#[component]
fn CountsChart(state: Memo<EventsState>) -> impl IntoView {
    let url = Memo::new(move |_| format!("/api/events/counts{}", state.get().first_page().to_query()));
    let data = LocalResource::new(move || load::<EventCounts>(url.get()));
    let shown = Memo::new(move |_| {
        let counts = data.get()?.ok()?;
        Shown::new(&counts)
    });
    let pointer = RwSignal::new(None::<f64>);
    let plot = NodeRef::<leptos::html::Div>::new();
    let order: [usize; SERIES] = std::array::from_fn(|index| index);

    let scale = Memo::new(move |_| Some(area::ticks(area::peak_series(&shown.get()?.months, &order), 4)));
    let bands = move || {
        let shown = shown.get()?;
        let (top, _) = scale.get()?;
        let bands = area::bands(&area::stack_series(&shown.months, order.map(|index| (index, index))), top);
        let fills = bands
            .iter()
            .map(|band| view! { <path class=format!("band series-{}", band.key) d=band.path.clone()></path> })
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
    let readout = move || {
        let shown = shown.get()?;
        let fraction = pointer.get()?;
        let index = area::month_at(fraction, shown.months.len())?;
        let month = shown.months.get(index)?;
        let rows = (0..SERIES)
            .rev()
            .filter(|series| month[*series] > 0.0)
            .map(|series| {
                view! {
                    <div class="value">
                        <span class=format!("key series-{series}")></span>
                        <strong>{format!("{}", month[series])}</strong>
                        " "
                        {series_label(series)}
                    </div>
                }
            })
            .collect_view();
        let (left, right) = crate::tooltip::beside(&format!("{:.2}%", fraction * 100.0), fraction > 0.7);
        Some(view! {
            <div class="tooltip" role="status" style:left=left style:right=right style:top="8px">
                <div class="meta">{shown.month(index).format("%B %Y").to_string()} " · " {format!("{}", month.iter().sum::<f64>())} " events"</div>
                {rows}
            </div>
        })
    };
    let status = move || match data.get() {
        None => Some("loading…".to_owned()),
        Some(Err(error)) => Some(error),
        Some(Ok(counts)) if counts.months.is_empty() => Some("No events match.".to_owned()),
        Some(Ok(_)) => None,
    };
    view! {
        <figure class="area">
            <figcaption class="row">
                <span class="label">"Events per month"</span>
                <span class="meta">"by category; the four smallest stack as Other"</span>
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
                <svg viewBox=format!("0 0 {WIDTH} {HEIGHT}") preserveAspectRatio="none" role="img" aria-label="Stacked area of events per month by category; the list below holds every event.">
                    {gridlines}
                    {bands}
                    {crosshair}
                </svg>
                <div class="y-axis" aria-hidden="true">{y_labels}</div>
                {readout}
                {move || status().map(|status| view! { <p class="plot-status meta">{status}</p> })}
            </div>
            <div class="axis" aria-hidden="true">{x_labels}</div>
        </figure>
    }
}

/// One page of the selected events, and the way to the pages either side.
#[component]
fn EventList(state: Memo<EventsState>) -> impl IntoView {
    let url = Memo::new(move |_| format!("/api/events{}", state.get().to_query()));
    let data = LocalResource::new(move || load::<EventsPage>(url.get()));
    move || match data.get() {
        None => view! { <p class="meta">"loading…"</p> }.into_any(),
        Some(Err(error)) => view! { <p class="meta">{error}</p> }.into_any(),
        Some(Ok(page)) => {
            let (newer, older) = neighbors(&page);
            let link = move |page: usize| format!("/events{}", state.get_untracked().at_page(page).to_query());
            let rows = page.rows.iter().cloned().map(|row| view! { <Row row /> }).collect_view();
            view! {
                <section class="events">
                    <p class="meta">{summary(&page)}</p>
                    <ol class="event-list">{rows}</ol>
                    <nav class="row pager" aria-label="Pages">
                        {newer.map(|page| view! { <a class="btn" href=link(page)>"← newer"</a> })}
                        {older.map(|page| view! { <a class="btn" href=link(page)>"older →"</a> })}
                    </nav>
                </section>
            }
            .into_any()
        }
    }
}

/// One event in the list.
#[component]
fn Row(row: EventRow) -> impl IntoView {
    let series = row.group.map_or(SERIES - 1, Group::series);
    let place = if row.units.is_empty() { row.place.clone() } else { row.units.join(", ") };
    let category = row.group.map_or("Other", Group::label);
    view! {
        <li class="event-row" class:retracted=row.retracted>
            <span class="meta date-cell">{row.date.to_string()}</span>
            <span class="row-label">
                <span class=format!("swatch series-{series}")></span>
                <span class="meta">{category}</span>
            </span>
            <span class="event-main">
                <a href=format!("/events/{}", row.number)>{row.title.clone().unwrap_or_else(|| format!("Event {}", row.number))}</a>
                <span class="meta">
                    {format!("EN {} · {place}", row.number)}
                    {row.state.clone().map(|state| format!(", {state}"))}
                    {(!row.sections.is_empty()).then(|| format!(" · {}", row.sections.join(", ")))}
                </span>
            </span>
            <span class="row">
                {row.scram.then(|| view! { <span class="chip scram">"scram"</span> })}
                {row.retracted.then(|| view! { <span class="chip">"retracted"</span> })}
            </span>
        </li>
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn page(total: usize, page: usize, pages: usize) -> EventsPage {
        EventsPage { total, page, pages, rows: Vec::new() }
    }

    #[test]
    fn the_series_are_seven_groups_and_other() {
        assert_eq!(
            (0..SERIES).map(series_label).collect::<Vec<_>>(),
            [
                "Power Reactor",
                "Agreement State",
                "General Information",
                "Fuel Cycle Facility",
                "Other Nuclear Material",
                "Non-Agreement State",
                "Part 21",
                "Other"
            ]
        );
    }

    #[test]
    fn pages_link_to_their_neighbors_and_say_where_they_are() {
        assert_eq!(neighbors(&page(120, 1, 3)), (None, Some(2)));
        assert_eq!(neighbors(&page(120, 2, 3)), (Some(1), Some(3)));
        assert_eq!(neighbors(&page(120, 3, 3)), (Some(2), None));
        assert_eq!(summary(&page(120, 2, 3)), "120 events, newest first; page 2 of 3.");
        assert_eq!(summary(&page(1, 1, 1)), "1 event.");
        assert_eq!(summary(&page(0, 1, 1)), "No events match.");
    }

    #[test]
    fn counts_become_the_areas_months() {
        let counts = EventCounts {
            first: NaiveDate::from_ymd_opt(2009, 8, 1),
            months: vec![[5, 0, 1, 0, 0, 0, 0, 3]],
        };
        let shown = Shown::new(&counts).unwrap_or_else(|| panic!("counts with a first month show"));
        assert_eq!(shown.months, [[5.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 3.0]]);
        assert_eq!(Shown::new(&EventCounts::default()), None);
    }
}
