//! Lucide icons from `icondata_lu`, drawn inline. No component library: one small component on
//! the instrument design language's rules (14px inline, `currentColor`, beside words).

use leptos::prelude::*;

/// An inline SVG icon that takes its color from the surrounding text.
#[component]
pub fn Icon(icon: icondata_core::Icon) -> impl IntoView {
    view! {
        <svg
            class="icon"
            viewBox=icon.view_box
            fill=icon.fill.unwrap_or("none")
            stroke=icon.stroke.unwrap_or("currentColor")
            stroke-width=icon.stroke_width.unwrap_or("1.5")
            stroke-linecap="square"
            stroke-linejoin="miter"
            aria-hidden="true"
            inner_html=icon.data
        ></svg>
    }
}
