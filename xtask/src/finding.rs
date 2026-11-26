//! `cargo xtask finding <url> <slug>` (PRD-0008 T-004, DESIGN.md Decision 46): writes the next
//! finding's stub from a view's URL, as copied from the browser. The query keeps only what the view
//! reads, in its canonical order, the figure is the URL's fragment, and the stub is dated today and
//! through the raw store's last status report. The note is written by hand.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, ensure};
use chrono::NaiveDate;

use downpower_app::state::ViewState;
use downpower_app::views::View;
use downpower_core::explorer::pairs;
use downpower_core::findings::render;
use downpower_ingest::raw::{RawStore, Series};

/// The findings directory.
const FINDINGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../findings");

pub fn run(data_dir: &Path, url: &str, slug: &str) -> anyhow::Result<()> {
    ensure!(data_dir.join("raw/manifest.jsonl").exists(), "no raw store under {}; run `downpower fetch` first", data_dir.display());
    let through = RawStore::open(data_dir)?
        .latest(Series::PowerStatus)?
        .into_iter()
        .rev()
        .find(|(_, record)| record.sha256.is_some())
        .map(|(date, _)| date)
        .context("the raw store holds no status report")?;
    let written = chrono::Local::now().date_naive();
    let dir = Path::new(FINDINGS);
    let (name, text) = stub(url, slug, next_number(dir)?, written, through)?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(name);
    ensure!(!path.exists(), "{} exists", path.display());
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "wrote {}: write its summary and note, then read it at /findings/{}",
        path.display(),
        path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default()
    );
    Ok(())
}

/// The stub for finding `number` from `url`: its file name and its text.
fn stub(url: &str, slug: &str, number: u16, written: NaiveDate, through: NaiveDate) -> anyhow::Result<(String, String)> {
    // Drop the scheme and host, then split off the fragment and the query.
    let path = url.split_once("://").map_or(url, |(_, rest)| rest.find('/').map_or("/", |slash| &rest[slash..]));
    let (path, figure) = path.split_once('#').map_or((path, None), |(path, figure)| (path, Some(figure)));
    let (path, query) = path.split_once('?').map_or((path, ""), |(path, query)| (path, query));
    let views = View::ALL.map(View::path).join(", ");
    let view = View::from_path(path).with_context(|| format!("no chart view at {path:?}; there are {views}"))?;
    let figures = view.figures().iter().map(|figure| figure.id).collect::<Vec<_>>().join(", ");
    let figure = figure
        .filter(|figure| view.figure(figure).is_some())
        .with_context(|| format!("end the URL with `#` and one of {path}'s figures: {figures}"))?;
    let read: Vec<String> = pairs(query)
        .into_iter()
        .filter(|(name, _)| view.reads().contains(&name.as_str()))
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    let query = ViewState::parse_query(&read.join("&")).to_query();
    let query = if query.is_empty() { String::new() } else { format!("query = {query:?}\n") };
    let mut title = slug.replace('-', " ");
    if let Some(first) = title.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    let name = format!("{number:04}-{slug}.md");
    let text = format!(
        "+++\ntitle = {title:?}\nsummary = \"TODO: one sentence for the index and the link preview.\"\nview = {:?}\n{query}figure = {figure:?}\n\
         written = {written}\nthrough = {through}\n+++\n\nTODO: the note.\n",
        view.path()
    );
    render::parse(&name, &text)?;
    Ok((name, text))
}

/// The number after the highest finding's in `dir`, or 1.
fn next_number(dir: &Path) -> anyhow::Result<u16> {
    if !dir.exists() {
        return Ok(1);
    }
    let mut highest = 0;
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let name = entry?.file_name();
        let number = name.to_str().and_then(|name| name.get(..4)).and_then(|digits| digits.parse::<u16>().ok());
        highest = highest.max(number.unwrap_or_default());
    }
    Ok(highest + 1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }

    fn error(url: &str) -> String {
        format!("{:#}", stub(url, "a", 1, day(26), day(25)).unwrap_err())
    }

    #[test]
    fn uat_findings_xtask_writes_a_stub_with_the_query_canonicalized() {
        let (name, text) = stub("http://127.0.0.1:3917/trends?families=forced,grid&from=1999-01-01#scram-rate", "scrams-fell", 4, day(26), day(25)).unwrap();
        assert_eq!(name, "0004-scrams-fell.md");
        assert_eq!(
            text,
            "+++\ntitle = \"Scrams fell\"\nsummary = \"TODO: one sentence for the index and the link preview.\"\nview = \"/trends\"\n\
             query = \"?from=1999-01-01&families=grid,forced\"\nfigure = \"scram-rate\"\nwritten = 2026-11-26\nthrough = 2026-11-25\n+++\n\nTODO: the note.\n"
        );
    }

    #[test]
    fn uat_findings_xtask_drops_what_the_view_doesnt_read_and_the_defaults() {
        let (_, text) = stub("https://downpower.twitchax.com/scrams?from=2010-01-01&layer=observed#trigger-rate", "a", 1, day(26), day(25)).unwrap();
        assert!(text.contains("query = \"?from=2010-01-01\"\n"), "{text}");
        let (_, text) = stub("/#lost", "a", 1, day(26), day(25)).unwrap();
        assert!(!text.contains("query"), "the defaults leave the query out: {text}");
        assert!(text.contains("view = \"/\"\n"), "{text}");
    }

    #[test]
    fn uat_findings_xtask_rejects_a_url_with_no_view_or_figure() {
        assert!(error("/events?scram=1#x").contains("/events"));
        assert!(error("/trends?from=1999-01-01").contains("scram-rate"), "no fragment: the message lists the figures");
        assert!(error("/trends#scrams").contains("scram-rate"));
    }

    #[test]
    fn uat_findings_xtask_numbers_after_the_highest() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(next_number(dir.path()).unwrap(), 1);
        for name in ["0001-a.md", "0003-b.md", "README.md"] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(next_number(dir.path()).unwrap(), 4);
        assert_eq!(next_number(&dir.path().join("missing")).unwrap(), 1);
    }
}
