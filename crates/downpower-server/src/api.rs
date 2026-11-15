//! The data the views fetch: the barcode buffer and the monthly lost energy per layer, one unit-day
//! for the barcode's hover, and what `/status` shows.
//! Both read the table `ledger:current` names, so a rebuild swaps them in with the table, and a
//! deploy that changes the record's shape keeps serving the old table until its rebuild lands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse as _, Response},
    routing::get,
};
use chrono::NaiveDate;
use serde::Serialize;

use downpower_core::barcode::Layer;

use crate::daily::{self, Run};
use crate::db::Store;
use crate::ledger::{self, Build, Packed};

/// The store, and the barcode last served per layer, reused while the stored tag matches it.
#[derive(Clone)]
struct Api {
    store: Store,
    barcodes: Arc<Mutex<HashMap<Layer, Packed>>>,
}

/// The `/api` routes over `store`.
pub fn router(store: Store) -> Router {
    let api = Api { store, barcodes: Arc::default() };
    Router::new()
        .route("/api/barcode/{layer}", get(barcode))
        .route("/api/lost/{layer}", get(lost))
        .route("/api/status", get(status))
        .route("/api/unit-day/{docket}/{date}", get(unit_day))
        .with_state(api)
}

/// A 503 while no build has ever landed: only a fresh volume's first rebuild leaves the site
/// without a table to read.
fn building() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, "30")], "the ledger is building").into_response()
}

fn failed(error: &anyhow::Error) -> Response {
    tracing::error!(error = format!("{error:#}"), "an API read failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "the read failed").into_response()
}

/// The barcode buffer for a layer, as `application/octet-stream`. Its weak `ETag` is a hash of the
/// bytes, so a browser revalidates for a 304 until the data changes. Weak, because the compression
/// layer may change the bytes on the wire. The buffer is cached by that tag, read from the store
/// on every request, so neither a rebuild nor an in-place update leaves a stale one behind.
async fn barcode(State(api): State<Api>, Path(layer): Path<String>, headers: HeaderMap) -> Response {
    let Some(layer) = Layer::from_slug(&layer) else {
        return (StatusCode::NOT_FOUND, "no such layer").into_response();
    };
    let table = match ledger::serving(&api.store).await {
        Ok(Some(table)) => table,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    let tag = match ledger::barcode_etag(&api.store, &table, layer).await {
        Ok(Some(tag)) => tag,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    let revalidated = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tags| tags.split(',').any(|candidate| candidate.trim().trim_start_matches("W/").trim_matches('"') == tag));
    if revalidated {
        return match weak(&tag) {
            Ok(etag) => (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response(),
            Err(error) => failed(&error),
        };
    }

    let cached = api.barcodes.lock().unwrap_or_else(PoisonError::into_inner).get(&layer).filter(|packed| packed.etag == tag).cloned();
    let packed = match cached {
        Some(packed) => packed,
        None => match ledger::barcode(&api.store, &table, layer).await {
            Ok(Some(packed)) => {
                api.barcodes.lock().unwrap_or_else(PoisonError::into_inner).insert(layer, packed.clone());
                packed
            }
            Ok(None) => return building(),
            Err(error) => return failed(&error),
        },
    };
    match weak(&packed.etag) {
        Ok(etag) => (
            [(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream")), (header::ETAG, etag)],
            Body::from(packed.bytes.into_inner()),
        )
            .into_response(),
        Err(error) => failed(&error),
    }
}

/// `tag` as a weak `ETag` header value.
fn weak(tag: &str) -> anyhow::Result<HeaderValue> {
    HeaderValue::from_str(&format!("W/\"{tag}\"")).map_err(|_| anyhow::anyhow!("an ETag that isn't a header value: {tag}"))
}

/// One unit-day from the current build, as JSON: 404 when the unit has no row that day.
async fn unit_day(State(api): State<Api>, Path((docket, date)): Path<(String, String)>) -> Response {
    let Ok(date) = date.parse::<NaiveDate>() else {
        return (StatusCode::BAD_REQUEST, "a date is YYYY-MM-DD").into_response();
    };
    let table = match ledger::serving(&api.store).await {
        Ok(Some(table)) => table,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    match ledger::unit_day(&api.store, &table, &docket, date).await {
        Ok(Some(day)) => Json(day).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no row for that unit and day").into_response(),
        Err(error) => failed(&error),
    }
}

/// The monthly reported power reduction by family for a layer, as JSON (the stacked area's data).
async fn lost(State(api): State<Api>, Path(layer): Path<String>) -> Response {
    let Some(layer) = Layer::from_slug(&layer) else {
        return (StatusCode::NOT_FOUND, "no such layer").into_response();
    };
    let table = match ledger::serving(&api.store).await {
        Ok(Some(table)) => table,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    match ledger::monthly(&api.store, &table, layer).await {
        Ok(Some(monthly)) => Json(monthly).into_response(),
        Ok(None) => building(),
        Err(error) => failed(&error),
    }
}

/// Runs `/status` lists.
const RUNS: u32 = 20;

/// What `/status` shows: the current build, with its summary, the table the views read, and the
/// newest runs. After a deploy that changed the build record, `build` is null until the rebuild
/// lands while `serving` still names the table the site reads.
#[derive(Serialize)]
struct Status {
    build: Option<Build>,
    serving: Option<String>,
    runs: Vec<Run>,
}

async fn status(State(api): State<Api>) -> Response {
    let (build, serving) = match tokio::try_join!(ledger::current(&api.store), ledger::serving(&api.store)) {
        Ok(read) => read,
        Err(error) => return failed(&error),
    };
    match daily::runs(&api.store, RUNS).await {
        Ok(runs) => Json(Status { build, serving, runs }).into_response(),
        Err(error) => failed(&error),
    }
}
