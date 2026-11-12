//! The real router, in-process: no socket, no built site.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::Body,
    http::{Request, StatusCode, header::CACHE_CONTROL},
};
use pretty_assertions::assert_eq;
use tower::ServiceExt as _;

use downpower_server::{router, tests::options};

#[tokio::test]
async fn health_is_ok_and_revalidates() {
    let response = router(options()).oneshot(Request::get("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
}
