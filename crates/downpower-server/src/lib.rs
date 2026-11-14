//! The axum server around the app: SSR routes, static assets from the site root, a health check,
//! the `/api` routes, and the cache policy. `router()` is a function of the options and the store, so
//! tests drive it in-process.

// Leptos view types nest deeply: laying out the home page's render and hydration futures overflows
// the default limit of 128 in a release build.
#![recursion_limit = "256"]

pub mod api;
pub mod cli;
pub mod daily;
pub mod db;
pub mod http_policy;
pub mod ledger;
pub mod tests;

use anyhow::Context as _;
use axum::{Router, middleware, routing::get};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes as _, generate_route_list};
use tower_http::compression::CompressionLayer;

use downpower_app::{App, shell};

/// The stack each runtime thread gets. `SurrealDB`'s futures nest deeply enough to overflow tokio's
/// 2 MiB default in a debug build, and a ledger rebuild runs on these threads.
pub const THREAD_STACK: usize = 8 << 20;

/// The multi-threaded runtime the binary and the Surreal tests run on.
///
/// # Errors
///
/// When the runtime cannot start its threads.
pub fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(THREAD_STACK)
        .enable_all()
        .build()
        .context("starting the runtime")
}

/// Builds the full router for `options`, with the `/api` routes reading `store`.
pub fn router(options: LeptosOptions, store: db::Store) -> Router {
    let routes = generate_route_list(App);
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .leptos_routes(&options, routes, {
            let options = options.clone();
            move || shell(options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(options)
        .merge(api::router(store))
        .layer(middleware::from_fn(http_policy::set_cache_control))
        // Fly's proxy doesn't compress. The barcode buffer is mostly runs of full-power days, so
        // gzip takes it from about a megabyte to a fraction of that.
        .layer(CompressionLayer::new())
}
