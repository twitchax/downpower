//! The downpower app: components, pages and the document shell. The server renders it (`ssr`)
//! and the browser bundle hydrates it (`hydrate`); this crate holds no server or browser glue.

// Leptos view types nest deeply: laying out the home page's render and hydration futures overflows
// the default limit of 128 in a release build.
#![recursion_limit = "256"]

pub mod area;
pub mod barcode;
#[cfg(feature = "hydrate")]
mod browser;
pub mod filters;
mod icon;
pub mod lost;
pub mod paint;
pub mod state;
pub mod status;

use leptos::prelude::*;
use leptos_meta::{HashedStylesheet, MetaTags, Title, provide_meta_context};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
    hooks::use_query_map,
};

use crate::barcode::BarcodeView;
use crate::filters::Filters;
pub use crate::icon::Icon;
use crate::lost::StackedArea;
use crate::state::ViewState;
use crate::status::StatusPage;

/// The HTML document the server renders around [`App`].
#[must_use]
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                // JetBrains Mono, the instrument design language's one face (OFL).
                <link rel="preconnect" href="https://fonts.googleapis.com" />
                <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;600&display=swap" />
                <AutoReload options=options.clone() />
                <HydrationScripts options=options.clone() />
                // Pairs with `hash-files = true`: links the content-hashed stylesheet.
                <HashedStylesheet options id="leptos" />
                <MetaTags />
            </head>
            // No `.grid-bg`: downpower drops the design language's background grid (docs/DESIGN.md).
            <body>
                <App />
            </body>
        </html>
    }
}

/// The root component: metadata and routes.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Title text="downpower" />
        <Router>
            <main>
                <nav class="nav">
                    <a href="/">"downpower"</a>
                    <a href="/status">"status"</a>
                </nav>
                <Routes fallback=|| view! { <p class="muted">"Page not found."</p> }>
                    <Route path=StaticSegment("") view=HomePage />
                    <Route path=StaticSegment("status") view=StatusPage />
                </Routes>
            </main>
        </Router>
    }
}

/// The landing page: the filters, the fleet barcode and the stacked area, all reading the URL's
/// state.
#[component]
fn HomePage() -> impl IntoView {
    let query = use_query_map();
    let state = Memo::new(move |_| query.with(|query| ViewState::from_query(|name| query.get(name))));

    view! {
        <header class="intro">
            <h1>"downpower"</h1>
            <p class="muted">
                "Why US nuclear reactors lose output: every unit, every day since 1999, colored by the cause the NRC's daily status report gives."
            </p>
        </header>
        <Filters state />
        <BarcodeView state />
        <StackedArea state />
    }
}
