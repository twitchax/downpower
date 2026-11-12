//! Cache policy. Hashed assets under `/pkg/` never change at a given URL, so browsers may keep them
//! forever; everything else (HTML, unhashed public files) must revalidate. Without both halves,
//! browsers heuristically cache a stale wasm across releases (ironpad).

use axum::{
    extract::Request,
    http::{HeaderValue, header::CACHE_CONTROL},
    middleware::Next,
    response::Response,
};

/// The `Cache-Control` value for a request path.
#[must_use]
pub fn cache_control(path: &str) -> &'static str {
    if path.starts_with("/pkg/") { "public, max-age=31536000, immutable" } else { "no-cache" }
}

/// Middleware that stamps every response with [`cache_control`] for its path.
pub async fn set_cache_control(request: Request, next: Next) -> Response {
    let value = cache_control(request.uri().path());
    let mut response = next.run(request).await;
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

#[cfg(test)]
mod unit {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn hashed_assets_are_immutable() {
        assert_eq!(cache_control("/pkg/app.0123abcd.wasm"), "public, max-age=31536000, immutable");
    }

    #[test]
    fn pages_revalidate() {
        assert_eq!(cache_control("/"), "no-cache");
        assert_eq!(cache_control("/robots.txt"), "no-cache");
    }
}
