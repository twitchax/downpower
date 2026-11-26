//! Parsing `findings/NNNN-slug.md` and rendering its note, for the app's build script.
//!
//! A finding is TOML frontmatter between `+++` lines, then the note in markdown:
//!
//! ```text
//! +++
//! title = "Scrams fell by more than half, and stayed down"
//! summary = "One sentence for the index and the link preview."
//! view = "/trends"
//! query = "?from=1999-01-01"
//! figure = "scram-rate"
//! written = 2026-11-26
//! through = 2026-11-25
//! +++
//!
//! The note.
//! ```
//!
//! Whether the view, its query and the figure exist is the app's check: only the app knows its
//! views.

use anyhow::{Context as _, bail, ensure};
use chrono::{Datelike as _, NaiveDate};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};
use serde::Deserialize;
use toml::value::Datetime;

use crate::Res;

/// The longest title and summary, in characters: past these a link preview truncates them.
pub const TITLE_MAX: usize = 100;
pub const SUMMARY_MAX: usize = 200;

/// A parsed finding, owned, before it is compiled in as a [`super::Finding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub number: u16,
    pub stem: String,
    pub title: String,
    pub summary: String,
    pub view: String,
    pub query: String,
    pub figure: String,
    pub written: NaiveDate,
    pub through: NaiveDate,
    pub html: String,
}

/// Parses one file, `name` being its file name (`0001-slug.md`).
///
/// # Errors
/// When the name, the frontmatter or the note breaks a rule in this module.
pub fn parse(name: &str, text: &str) -> Res<Parsed> {
    parse_named(name, text).with_context(|| format!("findings/{name}"))
}

fn parse_named(name: &str, text: &str) -> Res<Parsed> {
    let (number, stem) = file_name(name)?;
    let Some((front, note)) = text.strip_prefix("+++\n").and_then(|rest| rest.split_once("\n+++\n")) else {
        bail!("the frontmatter goes between two `+++` lines at the top of the file");
    };
    let front: Frontmatter = toml::from_str(front).context("the frontmatter")?;
    let written = local_date(front.written).context("`written`")?;
    let through = local_date(front.through).context("`through`")?;
    ensure!(through <= written, "`through` ({through}) is after `written` ({written})");
    one_line("title", &front.title, TITLE_MAX)?;
    one_line("summary", &front.summary, SUMMARY_MAX)?;
    ensure!(front.view.starts_with('/'), "`view` is a path starting with `/`, not {:?}", front.view);
    ensure!(
        front.query.is_empty() || front.query.starts_with('?'),
        "`query` starts with `?`, or is left out at the defaults, not {:?}",
        front.query
    );
    ensure!(!front.figure.trim().is_empty(), "`figure` names one of the view's figures");
    Ok(Parsed {
        number,
        stem,
        title: front.title,
        summary: front.summary,
        view: front.view,
        query: front.query,
        figure: front.figure,
        written,
        through,
        html: render(note)?,
    })
}

/// Parses every file, `(name, text)`, ordered by number.
///
/// # Errors
/// When a file doesn't parse, or two share a number.
pub fn parse_all(files: impl IntoIterator<Item = (String, String)>) -> Res<Vec<Parsed>> {
    let mut parsed = files.into_iter().map(|(name, text)| parse(&name, &text)).collect::<Res<Vec<_>>>()?;
    parsed.sort_by_key(|finding| finding.number);
    for pair in parsed.windows(2) {
        if let [first, second] = pair {
            ensure!(
                first.number != second.number,
                "findings/{}.md and findings/{}.md share the number {:04}",
                first.stem,
                second.stem,
                first.number
            );
        }
    }
    Ok(parsed)
}

/// The Rust expression for the compiled table, a `&[Finding]`, which the app `include!`s. Strings
/// are written with `Debug`, whose escapes are valid Rust literals.
#[must_use]
pub fn table(findings: &[Parsed]) -> String {
    let date = |date: NaiveDate| format!("::downpower_core::findings::date({}, {}, {})", date.year(), date.month(), date.day());
    let rows: Vec<String> = findings
        .iter()
        .map(|finding| {
            format!(
                "    ::downpower_core::findings::Finding {{ number: {}, stem: {:?}, title: {:?}, summary: {:?}, view: {:?}, \
                 query: {:?}, figure: {:?}, written: {}, through: {}, html: {:?} }},\n",
                finding.number,
                finding.stem,
                finding.title,
                finding.summary,
                finding.view,
                finding.query,
                finding.figure,
                date(finding.written),
                date(finding.through),
                finding.html
            )
        })
        .collect();
    format!("&[\n{}]", rows.concat())
}

/// The frontmatter, as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    title: String,
    summary: String,
    view: String,
    #[serde(default)]
    query: String,
    figure: String,
    written: Datetime,
    through: Datetime,
}

/// The number and stem of `NNNN-slug.md`: four digits from 0001, then lowercase words of letters
/// and digits joined by single hyphens.
fn file_name(name: &str) -> Res<(u16, String)> {
    let parts = name.strip_suffix(".md").and_then(|stem| Some((stem, stem.get(..4)?, stem.get(4..5)?, stem.get(5..)?)));
    let rule = "a finding is named `NNNN-slug.md`: four digits from 0001, then lowercase words joined by single hyphens";
    let Some((stem, digits, "-", slug)) = parts else { bail!("{rule}") };
    let number: u16 = digits.parse().ok().filter(|number| *number > 0 && digits.bytes().all(|byte| byte.is_ascii_digit())).context(rule)?;
    let word = |word: &str| !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    ensure!(slug.split('-').all(word), "{rule}");
    Ok((number, stem.to_owned()))
}

/// A TOML local date (`2026-11-26`), with no time or offset.
fn local_date(value: Datetime) -> Res<NaiveDate> {
    let Datetime {
        date: Some(date),
        time: None,
        offset: None,
    } = value
    else {
        bail!("a date with no time, not {value}")
    };
    NaiveDate::from_ymd_opt(i32::from(date.year), u32::from(date.month), u32::from(date.day)).with_context(|| format!("not a date: {value}"))
}

/// Checks a frontmatter field that a page shows on one line.
fn one_line(field: &str, text: &str, max: usize) -> Res<()> {
    ensure!(!text.trim().is_empty(), "`{field}` is empty");
    ensure!(!text.contains('\n'), "`{field}` is one line");
    let length = text.chars().count();
    ensure!(length <= max, "`{field}` runs {length} characters, past the {max} a link preview shows");
    Ok(())
}

/// The note as HTML: `CommonMark` with tables, footnotes and strikethrough, and no raw HTML, so a note
/// can't carry markup the page's styles don't expect. The page's `<h1>` is the title, so headings
/// start at `##`.
fn render(note: &str) -> Res<String> {
    ensure!(!note.trim().is_empty(), "the note is empty");
    let options = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
    let events: Vec<Event<'_>> = Parser::new_ext(note, options).collect();
    for event in &events {
        match event {
            Event::Html(html) | Event::InlineHtml(html) => bail!("raw HTML in the note ({:?}): write it in markdown", html.trim()),
            Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }) => {
                bail!("a level-one heading in the note: the page's title is its heading, so the note's start at `##`")
            }
            _ => {}
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    Ok(html)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const GOOD: &str = "+++
title = \"Scrams fell by more than half\"
summary = \"From about 105 per 100 unit-years to about 40.\"
view = \"/trends\"
query = \"?from=1999-01-01\"
figure = \"scram-rate\"
written = 2026-11-26
through = 2026-11-25
+++

Scrams *fell*.[^1]

| year | rate |
|---|---|
| 1999 | 105 |

[^1]: Event Notification scrams.
";

    fn with(old: &str, new: &str) -> String {
        assert!(GOOD.contains(old), "{old:?} is in the good finding");
        GOOD.replacen(old, new, 1)
    }

    fn error(name: &str, text: &str) -> String {
        match parse(name, text) {
            Ok(parsed) => panic!("{name} parsed: {parsed:?}"),
            Err(error) => format!("{error:#}"),
        }
    }

    #[test]
    fn a_finding_parses_and_its_note_renders_with_tables_and_footnotes() {
        let parsed = parse("0002-scrams-fell.md", GOOD).unwrap_or_else(|error| panic!("{error:#}"));
        assert_eq!(parsed.number, 2);
        assert_eq!(parsed.stem, "0002-scrams-fell");
        assert_eq!(parsed.title, "Scrams fell by more than half");
        assert_eq!(parsed.view, "/trends");
        assert_eq!(parsed.query, "?from=1999-01-01");
        assert_eq!(parsed.figure, "scram-rate");
        assert_eq!(parsed.written, NaiveDate::from_ymd_opt(2026, 11, 26).unwrap_or_default());
        assert_eq!(parsed.through, NaiveDate::from_ymd_opt(2026, 11, 25).unwrap_or_default());
        assert!(parsed.html.starts_with("<p>Scrams <em>fell</em>."), "{}", parsed.html);
        assert!(parsed.html.contains("<table>"), "{}", parsed.html);
        assert!(parsed.html.contains("footnote-definition"), "{}", parsed.html);
    }

    #[test]
    fn a_query_may_be_left_out_at_the_defaults() {
        let parsed = parse("0001-a.md", &with("query = \"?from=1999-01-01\"\n", "")).unwrap_or_else(|error| panic!("{error:#}"));
        assert_eq!(parsed.query, "");
    }

    #[test]
    fn uat_findings_checks_reject_a_missing_field_and_an_unknown_one() {
        assert!(error("0002-a.md", &with("figure = \"scram-rate\"\n", "")).contains("figure"));
        assert!(error("0002-a.md", &with("figure = \"scram-rate\"\n", "figure = \"scram-rate\"\nauthor = \"me\"\n")).contains("author"));
    }

    #[test]
    fn uat_findings_checks_reject_raw_html_in_a_note() {
        assert!(error("0002-a.md", &with("Scrams *fell*.", "Scrams <b>fell</b>.")).contains("raw HTML"));
        assert!(error("0002-a.md", &with("Scrams *fell*.", "<div>Scrams fell.</div>")).contains("raw HTML"));
    }

    #[test]
    fn uat_findings_checks_reject_a_duplicate_number() {
        let files = [("0002-a.md".to_owned(), GOOD.to_owned()), ("0002-b.md".to_owned(), GOOD.to_owned())];
        let error = parse_all(files).map(|parsed| parsed.len()).err().map(|error| format!("{error:#}")).unwrap_or_default();
        assert!(error.contains("0002"), "{error}");
    }

    #[test]
    fn the_name_must_be_a_number_and_a_kebab_slug() {
        for name in ["2-a.md", "0002_a.md", "0002-.md", "0002-A.md", "0002-a--b.md", "0002-a.txt", "0000-a.md", "0002-a-.md"] {
            assert!(error(name, GOOD).contains("NNNN-slug.md"), "{name}");
        }
    }

    #[test]
    fn the_frontmatter_rules() {
        assert!(error("0002-a.md", &GOOD.replacen("+++\n", "", 1)).contains("+++"));
        assert!(error("0002-a.md", &with("through = 2026-11-25", "through = 2026-11-27")).contains("through"));
        assert!(error("0002-a.md", &with("written = 2026-11-26", "written = 2026-11-26T08:00:00")).contains("written"));
        assert!(error("0002-a.md", &with("view = \"/trends\"", "view = \"trends\"")).contains("view"));
        assert!(error("0002-a.md", &with("query = \"?from=1999-01-01\"", "query = \"from=1999-01-01\"")).contains("query"));
        assert!(error("0002-a.md", &with("figure = \"scram-rate\"", "figure = \"\"")).contains("figure"));
        let long = "x".repeat(SUMMARY_MAX + 1);
        assert!(error("0002-a.md", &with("From about 105 per 100 unit-years to about 40.", &long)).contains("summary"));
        assert!(error("0002-a.md", &with("Scrams fell by more than half", &"x".repeat(TITLE_MAX + 1))).contains("title"));
    }

    #[test]
    fn the_note_is_markdown_under_the_page_title() {
        assert!(error("0002-a.md", &with("Scrams *fell*.", "# Scrams fell")).contains("heading"));
        let empty = GOOD.split_once("+++\n\n").map(|(front, _)| format!("{front}+++\n")).unwrap_or_default();
        assert!(error("0002-a.md", &empty).contains("note"));
    }

    #[test]
    fn the_table_is_a_slice_of_findings_with_escaped_strings() {
        let mut parsed = parse("0002-scrams-fell.md", GOOD).unwrap_or_else(|error| panic!("{error:#}"));
        parsed.title = "A \"quoted\" title".to_owned();
        parsed.html = "<p>a\nb</p>\n".to_owned();
        assert_eq!(
            table(&[parsed]),
            "&[\n    ::downpower_core::findings::Finding { number: 2, stem: \"0002-scrams-fell\", title: \"A \\\"quoted\\\" title\", \
             summary: \"From about 105 per 100 unit-years to about 40.\", view: \"/trends\", query: \"?from=1999-01-01\", \
             figure: \"scram-rate\", written: ::downpower_core::findings::date(2026, 11, 26), \
             through: ::downpower_core::findings::date(2026, 11, 25), html: \"<p>a\\nb</p>\\n\" },\n]"
        );
        assert_eq!(table(&[]), "&[\n]");
    }
}
