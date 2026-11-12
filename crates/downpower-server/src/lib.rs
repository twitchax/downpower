//! The axum server around the app: SSR routes, static assets from the site root, a health check,
//! and the cache policy. `router()` is a pure function of the options, so tests drive it in-process.

pub mod db;
pub mod http_policy;
pub mod tests;

use axum::{Router, middleware, routing::get};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes as _, generate_route_list};

use downpower_app::{App, shell};

/// Builds the full router for `options`.
pub fn router(options: LeptosOptions) -> Router {
    let routes = generate_route_list(App);
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .leptos_routes(&options, routes, {
            let options = options.clone();
            move || shell(options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(middleware::from_fn(http_policy::set_cache_control))
        .with_state(options)
}
