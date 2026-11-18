//! The events explorer's queries (PRD-0004 uat-002, uat-003) over the 38 events on the eight
//! fixture event pages, and its routes over a build of them. Among them:
//!
//! - EN 45286 (Crystal River 3) and 45290 (Browns Ferry 3), `M/R` scrams on 2009-08-24, and EN
//!   45957 (Vermont Yankee), an automatic scram on 2010-05-26.
//! - EN 35269 (Browns Ferry 2 and 3, 1999-01-15) and EN 54960 (2020-10-20), retracted.
//! - Three Hospital events on 2009-08-24, a Transportation Event, an ISFSI event and two research
//!   reactors, which the counts fold into Other.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header::ETAG, header::IF_NONE_MATCH};
use pretty_assertions::assert_eq;
use tower::ServiceExt as _;

use downpower_core::explorer::{EventCounts, EventsPage, EventsState, Group, PAGE};
use downpower_ingest::events::EventSet;
use downpower_ingest::ledger::{Catalog, Unknowns};
use downpower_ingest::raw::{RawStore, Series};
use downpower_ingest::units::Units;
use downpower_server::explorer::Explorer;
use downpower_server::ledger::{self, StoredEvent};
use downpower_server::tests::{block_on, fixture, options, put, raw_store};
use downpower_server::{db, router};

const PAGES: [&str; 8] = ["1999-01-15", "1999-02-02", "2002-10-14", "2009-08-24", "2010-05-26", "2012-01-09", "2020-10-20", "2026-02-11"];

/// Stores the fixture event pages in `store`.
fn put_pages(store: &RawStore) {
    for date in PAGES {
        put(store, Series::EventNotifications, date, Some(&fixture(&format!("events/{date}.html"))));
    }
}

fn explorer() -> Explorer {
    let tmp = tempfile::tempdir().unwrap();
    let store = RawStore::open(tmp.path()).unwrap();
    put_pages(&store);
    let set = EventSet::build(&store, &Catalog::embedded().unwrap().facilities, Unknowns::Fail).unwrap();
    let events = set.events.into_iter().map(|(number, event)| StoredEvent {
        identity: set.identities.get(&number).cloned(),
        page: set.pages.get(&number).copied(),
        event,
    });
    Explorer::new("event_1", events, &Units::catalog().unwrap())
}

/// The numbers `query` lists, every page of `per` events, in order.
fn numbers(explorer: &Explorer, query: &str, per: usize) -> Vec<u32> {
    let state = EventsState::parse_query(query);
    let mut numbers = Vec::new();
    for page in 1.. {
        let listed = explorer.page(&state.at_page(page), per);
        numbers.extend(listed.rows.iter().map(|row| row.number));
        if page >= listed.pages {
            break;
        }
    }
    numbers
}

fn all(explorer: &Explorer, query: &str) -> Vec<u32> {
    numbers(explorer, query, PAGE)
}

#[test]
fn uat_events_query_lists_every_event_newest_first() {
    let explorer = explorer();

    let listed = all(&explorer, "");

    assert_eq!(listed.len(), 38);
    assert_eq!(listed[..6], [58151, 58160, 58149, 55027, 54965, 54964]);
    assert_eq!(listed.last(), Some(&35267), "a day's events run from the highest number");
}

#[test]
fn uat_events_query_pages_are_stable_and_whole() {
    let explorer = explorer();
    let page = explorer.page(&EventsState::parse_query("page=4"), 10);

    assert_eq!((page.total, page.page, page.pages, page.rows.len()), (38, 4, 4, 8));
    assert_eq!(numbers(&explorer, "", 10), all(&explorer, ""));
    assert_eq!(explorer.page(&EventsState::parse_query("page=9"), 10).rows.len(), 0, "a page past the end is empty");
}

#[test]
fn uat_events_query_each_filter_selects_its_events() {
    let explorer = explorer();
    let cases: [(&str, &[u32]); 13] = [
        ("from=2020-01-01&to=2026-01-31", &[58160, 58149, 55027, 54965, 54964, 54962, 54960, 54959]),
        ("category=research-reactor", &[58160, 35267]),
        ("category=general-information", &[46009, 45459, 39314, 39290, 35273, 35271]),
        ("category=hospital,transportation", &[45295, 45294, 45293, 39282]),
        ("state=AL", &[45290, 35269]),
        ("facility=allegiance", &[45295, 45294]),
        ("facility=browns ferry 3", &[45290, 35269]),
        ("unit=05000296", &[45290, 35269]),
        ("cfr=50.72(b)(2)(iv)", &[45957, 45290, 45286]),
        ("cfr=50.72(b)(2)(iv)(A)", &[45290]),
        ("scram=1", &[45957, 45290, 45286]),
        ("q=camshaft stamp", &[55027]),
        ("state=AL&scram=1", &[45290]),
    ];
    for (query, expected) in cases {
        assert_eq!(all(&explorer, query), expected, "{query}");
    }
    assert_eq!(all(&explorer, "category=power-reactor").len(), 13);
    assert_eq!(all(&explorer, "class=non-emergency").len(), 37, "EN 39282 has no class");
}

#[test]
fn uat_events_query_rows_carry_what_the_list_shows() {
    let explorer = explorer();
    let page = explorer.page(&EventsState::parse_query("unit=05000296"), PAGE);

    let browns_ferry = &page.rows[1];
    assert_eq!(browns_ferry.number, 35269);
    assert_eq!(browns_ferry.group, Some(Group::PowerReactor));
    assert_eq!(browns_ferry.units, ["Browns Ferry 2", "Browns Ferry 3"]);
    assert!(browns_ferry.retracted && !browns_ferry.scram);
    assert_eq!(page.rows[0].sections, ["50.72(b)(2)(iv)(A)", "50.72(b)(2)(iv)(B)", "50.72(b)(3)(iv)(A)"]);
    assert!(page.rows[0].scram);
}

#[test]
fn uat_events_counts_follow_the_filters_and_fold_the_small_groups_into_other() {
    let explorer = explorer();

    let counts = explorer.counts(&EventsState::default());
    let august_2009 = (2009 - 1999) * 12 + 7;

    assert_eq!(counts.first.map(|first| first.to_string()).as_deref(), Some("1999-01-01"));
    assert_eq!(counts.months.len(), (2026 - 1999) * 12 + 2);
    assert_eq!(
        counts.months[august_2009],
        [5, 0, 1, 0, 0, 0, 0, 3],
        "Power Reactor, General Information, and three Hospital events as Other"
    );
    assert_eq!(counts.months.iter().flatten().sum::<u64>(), 38);

    for query in ["state=AL", "category=hospital,transportation", "scram=1", "from=2020-01-01"] {
        let state = EventsState::parse_query(query);
        let counts = explorer.counts(&state);
        assert_eq!(counts.months.iter().flatten().sum::<u64>(), explorer.page(&state, PAGE).total as u64, "{query}");
    }
    let other: EventCounts = explorer.counts(&EventsState::parse_query("category=hospital,transportation"));
    assert!(other.months.iter().all(|month| month[..7].iter().all(|count| *count == 0)), "both fold into Other");
}

async fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> axum::response::Response {
    let mut request = Request::get(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    router.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[test]
fn uat_events_query_routes_answer_from_the_serving_build() {
    block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        put_pages(&raw_store(tmp.path(), None));
        let store = db::open(tmp.path()).await.unwrap();
        let router = router(options(), store.clone());
        assert_eq!(get(&router, "/api/events", &[]).await.status(), StatusCode::SERVICE_UNAVAILABLE);
        ledger::rebuild(&store, tmp.path(), Unknowns::Fail).await.unwrap();

        let response = get(&router, "/api/events?state=AL&scram=1", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let tag = response.headers()[ETAG].to_str().unwrap().to_owned();
        let page: EventsPage = serde_json::from_value(json(response).await).unwrap();
        assert_eq!((page.total, page.rows[0].number), (1, 45290));
        assert_eq!(get(&router, "/api/events?state=AL&scram=1", &[(IF_NONE_MATCH.as_str(), &tag)]).await.status(), StatusCode::NOT_MODIFIED);

        let counts: EventCounts = serde_json::from_value(json(get(&router, "/api/events/counts?state=AL", &[]).await).await).unwrap();
        assert_eq!(counts.months.iter().flatten().sum::<u64>(), 2);

        let event = json(get(&router, "/api/events/45290", &[]).await).await;
        assert_eq!(
            event["nrc"].as_str(),
            Some("https://www.nrc.gov/documents-reports/document-collections/events-reports-associated-with/event-notification-reports/2009/20090824en#en45290"),
            "{event}"
        );
        assert_eq!((event["number"].as_u64(), event["category"].as_str()), (Some(45290), Some("power_reactor")), "{event}");
        assert_eq!(
            (event["identity"]["kind"].as_str(), event["identity"]["dockets"][0].as_str()),
            (Some("reactor"), Some("05000296")),
            "{event}"
        );
        assert_eq!((event["label"].as_str(), event["unit_names"][0].as_str()), (Some("Power Reactor"), Some("Browns Ferry 3")), "{event}");
        assert_eq!(event["units"][0]["scram"].as_str(), Some("manual_reactor"), "{event}");
        assert_eq!(get(&router, "/api/events/99999", &[]).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(get(&router, "/api/events/zebra", &[]).await.status(), StatusCode::BAD_REQUEST);
    });
}
