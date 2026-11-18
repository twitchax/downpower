//! The data the views fetch: the barcode buffer and the monthly lost energy per layer, one unit-day
//! for the barcode's hover, the events explorer's list, counts and events, and what `/status`
//! shows.
//! Both read the table `ledger:current` names, so a rebuild swaps them in with the table, and a
//! deploy that changes the record's shape keeps serving the old table until its rebuild lands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse as _, Response},
    routing::get,
};
use chrono::NaiveDate;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use downpower_core::barcode::Layer;
use downpower_core::explorer::{EventsState, PAGE};
use downpower_ingest::facilities::Identity;
use downpower_ingest::nrc::{self, Report};
use downpower_ingest::units::Units;

use crate::daily::{self, Run};
use crate::db::Store;
use crate::explorer::Explorer;
use crate::ledger::{self, Build, Packed, StoredEvent};

/// The store; the barcode last served per layer, reused while the stored tag matches it; and the
/// explorer's copy of the serving build's events, reused while the build is the same.
#[derive(Clone)]
struct Api {
    store: Store,
    barcodes: Arc<Mutex<HashMap<Layer, Packed>>>,
    explorer: Arc<Mutex<Option<Arc<Explorer>>>>,
}

/// The `/api` routes over `store`.
pub fn router(store: Store) -> Router {
    let api = Api {
        store,
        barcodes: Arc::default(),
        explorer: Arc::default(),
    };
    Router::new()
        .route("/api/barcode/{layer}", get(barcode))
        .route("/api/events", get(events))
        .route("/api/events/counts", get(event_counts))
        .route("/api/events/{number}", get(event))
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
    if revalidated(&headers, &tag) {
        return not_modified(&tag);
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

/// Whether the request's `If-None-Match` names `tag`.
fn revalidated(headers: &HeaderMap, tag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tags| tags.split(',').any(|candidate| candidate.trim().trim_start_matches("W/").trim_matches('"') == tag))
}

fn not_modified(tag: &str) -> Response {
    match weak(tag) {
        Ok(etag) => (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response(),
        Err(error) => failed(&error),
    }
}

/// The explorer over the serving build's events, or `None` before any build: the cached one while
/// the build is the same, else one read from its event table. A build is the same while its table
/// and `built_at` are, so an update in place, which rewrites events in the same table, reads them
/// again.
async fn explorer(api: &Api) -> anyhow::Result<Option<Arc<Explorer>>> {
    let Some(table) = ledger::serving(&api.store).await? else {
        return Ok(None);
    };
    let built_at = ledger::current(&api.store).await?.filter(|build| build.table == table).map(|build| build.built_at.to_rfc3339());
    let key = format!("{table}@{}", built_at.unwrap_or_default());
    let cached = api.explorer.lock().unwrap_or_else(PoisonError::into_inner).as_ref().filter(|explorer| explorer.key() == key).cloned();
    if let Some(explorer) = cached {
        return Ok(Some(explorer));
    }
    let explorer = Arc::new(Explorer::new(key, ledger::events(&api.store, &table).await?, &Units::catalog()?));
    *api.explorer.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&explorer));
    Ok(Some(explorer))
}

/// A tag for the answer to `query` from `explorer`'s build.
fn query_tag(explorer: &Explorer, kind: &str, query: &str) -> String {
    hex::encode(&Sha256::digest(format!("{}\n{kind}\n{query}", explorer.key()))[..8])
}

/// `body` as JSON under a weak `ETag` from `tag`, or a 304 when the request already holds it.
fn tagged(headers: &HeaderMap, tag: &str, body: impl Serialize) -> Response {
    if revalidated(headers, tag) {
        return not_modified(tag);
    }
    match weak(tag) {
        Ok(etag) => ([(header::ETAG, etag)], Json(body)).into_response(),
        Err(error) => failed(&error),
    }
}

/// A page of the events the query's filters select, newest first, as JSON.
async fn events(State(api): State<Api>, uri: Uri, headers: HeaderMap) -> Response {
    let state = EventsState::parse_query(uri.query().unwrap_or_default());
    let explorer = match explorer(&api).await {
        Ok(Some(explorer)) => explorer,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    let tag = query_tag(&explorer, "page", &state.to_query());
    tagged(&headers, &tag, explorer.page(&state, PAGE))
}

/// The events the query's filters select, counted by month and series, as JSON.
async fn event_counts(State(api): State<Api>, uri: Uri, headers: HeaderMap) -> Response {
    let state = EventsState::parse_query(uri.query().unwrap_or_default()).first_page();
    let explorer = match explorer(&api).await {
        Ok(Some(explorer)) => explorer,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    let tag = query_tag(&explorer, "counts", &state.to_query());
    tagged(&headers, &tag, explorer.counts(&state))
}

/// An event as `/api/events/{number}` answers it: as stored, with the NRC page that shows it, its
/// category's published label, and a reactor event's units by catalog name.
#[derive(Serialize)]
struct EventDetail {
    #[serde(flatten)]
    stored: StoredEvent,
    /// The event's anchor on the NRC page it came from.
    nrc: Option<String>,
    label: &'static str,
    unit_names: Vec<String>,
}

/// One event from the serving build, as stored, with its NRC link: 400 for a number that isn't
/// one, 404 for an event it doesn't hold.
async fn event(State(api): State<Api>, Path(number): Path<String>) -> Response {
    let Ok(number) = number.parse::<u32>() else {
        return (StatusCode::BAD_REQUEST, "an event number is digits").into_response();
    };
    let table = match ledger::serving(&api.store).await {
        Ok(Some(table)) => table,
        Ok(None) => return building(),
        Err(error) => return failed(&error),
    };
    match ledger::event(&api.store, &table, number).await {
        Ok(Some(stored)) => {
            let units = match Units::catalog() {
                Ok(units) => units,
                Err(error) => return failed(&error),
            };
            let unit_names = match &stored.identity {
                Some(Identity::Reactor { dockets }) => dockets
                    .iter()
                    .map(|docket| {
                        units
                            .all()
                            .iter()
                            .find(|unit| unit.docket == *docket)
                            .map_or_else(|| docket.as_str().to_owned(), |unit| unit.name.clone())
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Json(EventDetail {
                nrc: stored.page.map(|page| format!("{}{}#en{number}", nrc::BASE_URL, Report::EventNotifications.path(page))),
                label: stored.event.category.label(),
                unit_names,
                stored,
            })
            .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, "no such event").into_response(),
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
