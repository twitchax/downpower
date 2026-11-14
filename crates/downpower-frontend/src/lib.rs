//! The wasm entry point. cargo-leptos builds this crate for the browser; it only hydrates the
//! server-rendered app. All UI lives in `downpower_app`.

// Leptos view types nest deeply: laying out the home page's render and hydration futures overflows
// the default limit of 128 in a release build.
#![recursion_limit = "256"]

/// Called by the generated JS once the bundle loads.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(downpower_app::App);
}
