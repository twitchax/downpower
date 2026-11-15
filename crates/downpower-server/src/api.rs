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

/// The store, and the barcodes last served, keyed by layer with the table they came from.
#[derive(Clone)]
struct Api {
    store: Store,
    barcodes: Arc<Mutex<HashMap<Layer, (String, Packed)>>>,
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
/// layer may change the bytes on the wire.
async fn barcode(State(api): State<Api>, Path(layer): Path<String>, headers: HeaderMap) -> Response {
    let Some(layer) = Layer::from_slug(&layer) else {
        return (StatusCode::NOT_FOUND, "no such layer").into_response();
    };
    let table = match ledger::serving(&api.store).await {
        Ok(Some(table)) => table,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    let cached = api
        .barcodes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&layer)
        .filter(|(of, _)| *of == table)
        .map(|(_, packed)| packed.clone());
    let packed = match cached {
        Some(packed) => packed,
        None => match ledger::barcode(&api.store, &table, layer).await {
            Ok(Some(packed)) => {
                api.barcodes.lock().unwrap_or_else(PoisonError::into_inner).insert(layer, (table, packed.clone()));
                packed
            }
            Ok(None) => return building(),
            Err(error) => return failed(&error),
        },
    };

    let etag = format!("W/\"{}\"", packed.etag);
    let matches = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tags| tags.split(',').any(|tag| tag.trim().trim_start_matches("W/") == etag.trim_start_matches("W/")));
    let Ok(etag) = HeaderValue::from_str(&etag) else {
        return failed(&anyhow::anyhow!("an ETag that isn't a header value: {etag}"));
    };
    if matches {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream")), (header::ETAG, etag)],
        Body::from(packed.bytes.into_inner()),
    )
        .into_response()
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

/// What `/status` shows: the current build, with its summary, and the newest runs.
#[derive(Serialize)]
struct Status {
    build: Option<Build>,
    runs: Vec<Run>,
}

async fn status(State(api): State<Api>) -> Response {
    let build = match ledger::current(&api.store).await {
        Ok(build) => build,
        Err(error) => return failed(&error),
    };
    match daily::runs(&api.store, RUNS).await {
        Ok(runs) => Json(Status { build, runs }).into_response(),
        Err(error) => failed(&error),
    }
}
