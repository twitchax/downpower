//! The downpower app: components, pages and the document shell. The server renders it (`ssr`)
//! and the browser bundle hydrates it (`hydrate`); this crate holds no server or browser glue.

mod icon;

use leptos::prelude::*;
use leptos_meta::{HashedStylesheet, MetaTags, Title, provide_meta_context};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

pub use crate::icon::Icon;

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
                <Routes fallback=|| view! { <p class="muted">"Page not found."</p> }>
                    <Route path=StaticSegment("") view=HomePage />
                </Routes>
            </main>
        </Router>
    }
}

/// The landing page: one panel, one live counter, one primary action.
#[component]
fn HomePage() -> impl IntoView {
    let count = RwSignal::new(0_u32);

    view! {
        <section class="panel framed">
            <div class="label">"downpower"</div>
            <h1>"It works."</h1>
            <p class="muted">"Edit crates/downpower-app/src/lib.rs and the page reloads."</p>
            <div class="row">
                <button class="btn primary" on:click=move |_| *count.write() += 1>
                    <Icon icon=icondata_lu::LuPlus />
                    "Count"
                </button>
                <span class="meta">{move || clicks(count.get())}</span>
            </div>
        </section>
    }
}

/// The counter's status sentence: state in words, never a bare number.
fn clicks(count: u32) -> String {
    match count {
        0 => "not clicked yet".to_owned(),
        1 => "clicked once".to_owned(),
        n => format!("clicked {n} times"),
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn counter_states_its_count_in_words() {
        assert_eq!(clicks(0), "not clicked yet");
        assert_eq!(clicks(1), "clicked once");
        assert_eq!(clicks(7), "clicked 7 times");
    }
}
