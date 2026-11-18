//! `cargo xtask oracle`: cuts the events of the NRC's raw data file out of their stored pages, into
//! the fixture `uat_events_oracle` holds the file against, so CI needs no raw store (PRD-0003 T-009).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, ensure};
use chrono::{DateTime, NaiveDate, Utc};

use downpower_ingest::events::{self, Parser};
use downpower_ingest::raw::{Hash, RawStore, Series};

/// `event-notification-rpt-lastmonth.txt` as nrc.gov served it, last modified 2025-08-05.
const FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crates/downpower-ingest/tests/fixtures/events/oracle/event-notification-rpt-lastmonth.txt"
);
/// The file's events, each cut byte for byte from its newest stored page.
const PAGES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/downpower-ingest/tests/fixtures/events/oracle/events.html");

pub fn run(data_dir: &Path) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let store = RawStore::open(data_dir)?;
    let numbers = numbers(&fs::read_to_string(FILE).with_context(|| format!("reading {FILE}"))?)?;
    let wanted: BTreeSet<u32> = numbers.iter().copied().collect();

    // Each event's page: the newest fetch, then the later date, as the event set takes it.
    let mut newest: BTreeMap<u32, (DateTime<Utc>, NaiveDate, Hash)> = BTreeMap::new();
    for (date, record) in store.latest(Series::EventNotifications)? {
        let Some(hash) = record.sha256 else { continue };
        let page = String::from_utf8(store.get(&hash)?).with_context(|| format!("the event page for {date} is not UTF-8"))?;
        for number in ids(&page).filter(|number| wanted.contains(number)) {
            if newest.get(&number).is_none_or(|(fetched_at, held, _)| (*fetched_at, *held) < (record.fetched_at, date)) {
                newest.insert(number, (record.fetched_at, date, hash.clone()));
            }
        }
    }
    let missing: Vec<&u32> = wanted.iter().filter(|number| !newest.contains_key(number)).collect();
    ensure!(missing.is_empty(), "no stored page holds events {missing:?}");

    let parser = Parser::new()?;
    let (mut blocks, mut pages) = (String::new(), BTreeSet::new());
    for number in &numbers {
        let (fetched_at, date, hash) = &newest[number];
        pages.insert(date);
        let page = String::from_utf8(store.get(hash)?).with_context(|| format!("the event page for {date} is not UTF-8"))?;
        let block = cut(&page, *number).with_context(|| format!("event {number} on the {date} page"))?;
        let whole = parser.parse(page.as_bytes()).with_context(|| format!("the event page for {date}"))?;
        let alone = parser
            .parse(document(&[*number], block)?.as_bytes())
            .with_context(|| format!("event {number} cut from the {date} page"))?;
        ensure!(
            whole.iter().find(|event| event.number == *number) == alone.first(),
            "event {number} parses differently cut from the {date} page"
        );
        write!(blocks, "<!-- en{number}: the {date} page, fetched {} -->\n{block}\n", fetched_at.to_rfc3339())?;
    }
    let html = document(&numbers, &blocks)?;

    let parsed: Vec<u32> = events::parse(html.as_bytes())?.iter().map(|event| event.number).collect();
    ensure!(parsed == numbers, "the fixture parses to {parsed:?}, not the file's {numbers:?}");
    fs::write(PAGES, html).with_context(|| format!("writing {PAGES}"))?;
    println!("{} events from {} pages written to the oracle fixture", numbers.len(), pages.len());
    Ok(())
}

/// A page the parser reads: the event list's summary, listing `numbers`, and then `blocks`.
fn document(numbers: &[u32], blocks: &str) -> anyhow::Result<String> {
    let mut html = String::from("<main><div class=\"nrc-event-report-day\">\n<div class=\"event-summary\"></div>\n<p class=\"event-summary\">");
    for number in numbers {
        write!(html, "<a href=\"#en{number}\">{number}</a>")?;
    }
    write!(html, "</p>\n{blocks}</div></main>\n")?;
    Ok(html)
}

/// The `En No` column of the file, in its order.
fn numbers(file: &str) -> anyhow::Result<Vec<u32>> {
    let mut lines = file.split("\r\n").filter(|line| !line.is_empty());
    let column = lines.next().and_then(|header| header.split('|').position(|name| name == "En No")).context("no En No column")?;
    lines
        .map(|line| {
            let field = line.split('|').nth(column).with_context(|| format!("a row with no En No: {line:.60}"))?;
            field.parse().with_context(|| format!("En No {field:?}"))
        })
        .collect()
}

/// The event numbers of a page's blocks.
fn ids(page: &str) -> impl Iterator<Item = u32> + '_ {
    page.split(GRID).skip(1).filter_map(|rest| rest.split_once('"').and_then(|(number, _)| number.parse().ok()))
}

const GRID: &str = "<div class=\"grid border\" id=\"en";

/// Event `number`'s markup on `page`: its field grid, unit table and text, through the text's
/// closing tag. The text's div holds only text and `<br>`s (the parser fails on any other element),
/// so the first `</div>` after its heading closes it.
fn cut(page: &str, number: u32) -> anyhow::Result<&str> {
    let start = page.find(&format!("{GRID}{number}\">")).context("no block")?;
    let heading = start + page[start..].find("<b>Event Text</b>").context("no event text")?;
    let end = page[heading..].find("</div>").context("the event text never closes")?;
    Ok(&page[start..heading + end + "</div>".len()])
}
