//! The wasm entry point. cargo-leptos builds this crate for the browser; it only hydrates the
//! server-rendered app. All UI lives in `downpower_app`.

/// Called by the generated JS once the bundle loads.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(downpower_app::App);
}
