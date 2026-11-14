//! The one row of filters above the charts (DESIGN.md Decision 24): the layer, the date range, and
//! the family legend, which filters too. Every control writes the URL, so each view is a permalink.

use chrono::{Datelike as _, NaiveDate};
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;

use downpower_core::barcode::Layer;
use downpower_core::{Families, Family};

use crate::state::ViewState;

/// Today, for the range presets. Only the browser runs a click handler.
fn today() -> NaiveDate {
    #[cfg(feature = "hydrate")]
    return crate::browser::today();
    #[cfg(not(feature = "hydrate"))]
    NaiveDate::default()
}

/// The first day of a preset covering the last `years` years up to `today`.
fn years_back(today: NaiveDate, years: i32) -> NaiveDate {
    today.with_year(today.year() - years).or_else(|| NaiveDate::from_ymd_opt(today.year() - years, 2, 28)).unwrap_or(today)
}

/// Writes a state to the URL, replacing the current history entry.
fn navigator() -> impl Fn(ViewState) + Clone {
    let navigate = use_navigate();
    move |next: ViewState| {
        navigate(
            &format!("/{}", next.to_query()),
            NavigateOptions {
                replace: true,
                scroll: false,
                ..NavigateOptions::default()
            },
        );
    }
}

#[component]
pub fn Filters(state: Memo<ViewState>) -> impl IntoView {
    view! {
        <div class="filters">
            <Layers state />
            <Range state />
        </div>
        <Legend state />
    }
}

/// Attributed or observed (Decision 20).
#[component]
fn Layers(state: Memo<ViewState>) -> impl IntoView {
    let go = navigator();
    let buttons = Layer::ALL
        .map(|layer| {
            let go = go.clone();
            let (label, title) = match layer {
                Layer::Attributed => ("Attributed", "Causes as attributed: a day that only says power is rising inherits its unit's last cause"),
                Layer::Observed => ("Observed", "Only the causes each day's own reason names"),
            };
            view! {
                <button
                    class="btn"
                    aria-pressed=move || (state.get().layer == layer).to_string()
                    title=title
                    on:click=move |_| go(ViewState { layer, ..state.get_untracked() })
                >
                    {label}
                </button>
            }
        })
        .collect_view();
    view! { <div class="row" role="group" aria-label="Layer">{buttons}</div> }
}

/// Presets for the last few years, and the dates themselves.
#[component]
fn Range(state: Memo<ViewState>) -> impl IntoView {
    let go = navigator();
    let presets = [("All", None), ("10 yr", Some(10)), ("5 yr", Some(5)), ("1 yr", Some(1))]
        .map(|(label, years)| {
            let go = go.clone();
            view! {
                <button
                    class="btn"
                    aria-pressed=move || {
                        let view = state.get();
                        (years.is_none() && view.from.is_none() && view.to.is_none()).to_string()
                    }
                    on:click=move |_| go(ViewState { from: years.map(|years| years_back(today(), years)), to: None, ..state.get_untracked() })
                >
                    {label}
                </button>
            }
        })
        .collect_view();
    let date = |label: &'static str, get: fn(&ViewState) -> Option<NaiveDate>, set: fn(&mut ViewState, Option<NaiveDate>)| {
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
                        go(next);
                    }
                />
            </label>
        }
    };
    let from = date("from", |view| view.from, |view, date| view.from = date);
    let to = date("to", |view| view.to, |view, date| view.to = date);
    view! { <div class="row" role="group" aria-label="Range">{presets} {from} {to}</div> }
}

/// The family legend, each entry a toggle: a hidden family recedes to gray in every chart.
#[component]
fn Legend(state: Memo<ViewState>) -> impl IntoView {
    let go = navigator();
    let items = Family::STACK
        .map(|family| {
            let go = go.clone();
            view! {
                <button
                    class="legend-item"
                    aria-pressed=move || state.get().families.contains(family).to_string()
                    on:click=move |_| {
                        let view = state.get_untracked();
                        go(ViewState { families: view.families.toggled(family), ..view });
                    }
                >
                    <span class=format!("swatch family-{}", family.slug())></span>
                    {family.label()}
                </button>
            }
        })
        .collect_view();
    let reset = move || {
        let go = go.clone();
        (state.get().families != Families::ALL).then(|| {
            view! { <button class="btn" on:click=move |_| go(ViewState { families: Families::ALL, ..state.get_untracked() })>"Show all"</button> }
        })
    };
    view! {
        <div class="legend" role="group" aria-label="Cause families: press one to hide or show it">
            {items}
            {reset}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_preset_counts_back_whole_years_and_survives_a_leap_day() {
        let date = |text: &str| text.parse::<NaiveDate>().unwrap_or_default();
        assert_eq!(years_back(date("2026-11-14"), 10), date("2016-11-14"));
        assert_eq!(years_back(date("2024-02-29"), 1), date("2023-02-28"));
    }
}
