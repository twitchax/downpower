//! The Power Reactor Status Report parser. Every page since 1999 renders through one Drupal
//! template: a table per NRC region, with the columns
//! `Unit | Power | Down | Reason or Comment | Change in Report (*) | Scrams (#)`, or only
//! `Unit | Power` while the page is inside its 28-day hold. Anything else fails the parse, so a
//! template change stops the backfill instead of quietly dropping rows.

use anyhow::{Context as _, anyhow, bail, ensure};
use chrono::NaiveDate;
use scraper::{ElementRef, Html, Selector};

use crate::Res;

/// The report itself, inside the site chrome.
const CONTENT: &str = "div.nrc-power-reactor-status-report-day";
const NO_DATA: &str = "No data available for this date.";

/// The NRC's four regions, one table each.
const REGIONS: u8 = 4;

const FULL: [&str; 6] = ["Unit", "Power", "Down", "Reason or Comment", "Change in Report (*)", "Scrams (#)"];
const HELD: [&str; 2] = ["Unit", "Power"];

/// One parsed status page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusPage {
    /// The page reads "No data available for this date." Six report dates (1999-05-08,
    /// 2013-07-18, 2013-12-21, and 2015-12-02 to 12-04) are permanent gaps, and a day reads this
    /// way until its page is published.
    NoData,
    /// Every unit on the page, region by region, in page order.
    Report(Vec<Row>),
}

/// One unit's line in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The NRC region, 1 to 4.
    pub region: u8,
    /// The unit as the page names it. The unit spine maps it to a docket (PRD-0002 T-004).
    pub unit: String,
    /// The Power column, 0 to 100. What it measures is still open (DESIGN.md, "Still open").
    pub power: u8,
    /// The four columns a page gains when its 28-day hold ends, `None` until then.
    pub detail: Option<Detail>,
}

/// The columns past `Unit | Power`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    /// The date the unit went down, where the page gives one.
    pub down: Option<NaiveDate>,
    /// The reason or comment as written, with CRLFs made LFs and the ends trimmed; `None` when
    /// blank. Splitting it into tags is the classifier's job.
    pub reason: Option<String>,
    /// The `*` in "Change in Report (*)".
    pub changed: bool,
    /// The "Scrams (#)" count, with a blank read as 0. The NRC fills it in from mid-2013 on.
    pub scrams: u8,
}

#[derive(Debug, Clone, Copy)]
enum Layout {
    Full,
    Held,
}

impl Layout {
    fn of(header: &[String]) -> Res<Self> {
        if header.iter().eq(FULL.iter()) {
            Ok(Self::Full)
        } else if header.iter().eq(HELD.iter()) {
            Ok(Self::Held)
        } else {
            bail!("unknown header {header:?}")
        }
    }

    const fn width(self) -> usize {
        match self {
            Self::Full => FULL.len(),
            Self::Held => HELD.len(),
        }
    }
}

struct Selectors {
    content: Selector,
    region: Selector,
    header: Selector,
    row: Selector,
    cell: Selector,
}

impl Selectors {
    fn new() -> Res<Self> {
        let parse = |css: &str| Selector::parse(css).map_err(|error| anyhow!("selector {css:?}: {error}"));
        Ok(Self {
            content: parse(CONTENT)?,
            region: parse(r#"div[id^="region-"]"#)?,
            header: parse("thead th")?,
            row: parse("tbody tr")?,
            cell: parse("td")?,
        })
    }
}

/// Parses one status page, as stored in the raw store.
///
/// # Errors
///
/// When the page is not UTF-8 or departs from the template: no report block, a block with neither
/// tables nor the no-data notice, regions missing or out of order, an unknown header, a row whose
/// width doesn't match its header, or a cell that doesn't parse.
pub fn parse(page: &[u8]) -> Res<StatusPage> {
    let text = std::str::from_utf8(page).context("a status page is not UTF-8")?;
    // Pages from 2025 on put a lone NUL in a blank reason cell. html5ever drops it, as the HTML
    // spec says a parser must inside a table cell.
    let html = Html::parse_document(text);
    let selectors = Selectors::new()?;

    let mut blocks = html.select(&selectors.content);
    let (Some(content), None) = (blocks.next(), blocks.next()) else {
        bail!("expected exactly one {CONTENT}");
    };

    let tables: Vec<_> = content.select(&selectors.region).collect();
    if tables.is_empty() {
        ensure!(content.text().collect::<String>().contains(NO_DATA), "the report has neither region tables nor {NO_DATA:?}");
        return Ok(StatusPage::NoData);
    }
    ensure!(tables.len() == usize::from(REGIONS), "expected {REGIONS} region tables, found {}", tables.len());

    let mut rows = Vec::new();
    for (region, table) in (1..=REGIONS).zip(tables) {
        let id = table.value().id().unwrap_or_default();
        ensure!(id == format!("region-{region}"), "expected region-{region} next, found {id:?}");
        parse_region(region, table, &selectors, &mut rows).with_context(|| format!("region {region}"))?;
    }
    Ok(StatusPage::Report(rows))
}

fn parse_region(region: u8, table: ElementRef<'_>, selectors: &Selectors, rows: &mut Vec<Row>) -> Res<()> {
    let header: Vec<String> = table.select(&selectors.header).map(cell_text).collect();
    let layout = Layout::of(&header)?;
    for (index, tr) in table.select(&selectors.row).enumerate() {
        let mut cells: Vec<String> = tr.select(&selectors.cell).map(cell_text).collect();
        let row = parse_row(region, layout, &mut cells).with_context(|| format!("row {}", index + 1))?;
        rows.push(row);
    }
    Ok(())
}

fn parse_row(region: u8, layout: Layout, cells: &mut [String]) -> Res<Row> {
    let (unit, power, detail) = match (layout, cells) {
        (Layout::Held, [unit, power]) => (unit, power, None),
        (Layout::Full, [unit, power, down, reason, changed, scrams]) => {
            let detail = parse_detail(down, std::mem::take(reason), changed, scrams).with_context(|| format!("unit {unit:?}"))?;
            (unit, power, Some(detail))
        }
        (_, cells) => bail!("{} cells under a {}-column header", cells.len(), layout.width()),
    };
    ensure!(!unit.is_empty(), "a row with no unit name");
    let parsed: u8 = power.parse().with_context(|| format!("unit {unit:?}: power {power:?}"))?;
    ensure!(parsed <= 100, "unit {unit:?}: power {parsed} is over 100");
    Ok(Row {
        region,
        unit: std::mem::take(unit),
        power: parsed,
        detail,
    })
}

fn parse_detail(down: &str, reason: String, changed: &str, scrams: &str) -> Res<Detail> {
    let down = match down {
        "" => None,
        date => Some(NaiveDate::parse_from_str(date, "%Y-%m-%d").with_context(|| format!("down date {date:?}"))?),
    };
    let changed = match changed {
        "" => false,
        "*" => true,
        mark => bail!("change mark {mark:?}"),
    };
    let scrams = match scrams {
        "" => 0,
        count => count.parse().with_context(|| format!("scram count {count:?}"))?,
    };
    Ok(Detail {
        down,
        reason: (!reason.is_empty()).then_some(reason),
        changed,
        scrams,
    })
}

/// A cell's text with entities decoded, CRLFs made LFs and the ends trimmed. Older pages pad every
/// cell to a fixed width.
fn cell_text(cell: ElementRef<'_>) -> String {
    let text: String = cell.text().collect();
    text.trim().replace("\r\n", "\n")
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::fmt::Write as _;

    use pretty_assertions::assert_eq;

    use super::*;

    /// A region table: its div id, its header, and its rows.
    type Table<'a> = (&'a str, &'a [&'a str], &'a [&'a [&'a str]]);

    /// A page in the live template.
    fn page(tables: &[Table<'_>]) -> String {
        let mut html = r#"<!DOCTYPE html><html><body><div class="nrc-power-reactor-status-report-day">"#.to_owned();
        for (id, header, rows) in tables {
            write!(html, r#"<div id="{id}"><h3>Region</h3><table><thead><tr>"#).unwrap();
            for cell in *header {
                write!(html, "<th>{cell}</th>").unwrap();
            }
            html.push_str("</tr></thead><tbody>");
            for row in *rows {
                html.push_str("<tr>");
                for cell in *row {
                    write!(html, "<td>{cell}</td>").unwrap();
                }
                html.push_str("</tr>");
            }
            html.push_str("</tbody></table></div>");
        }
        html + "</div></body></html>"
    }

    /// Four regions, the first holding `rows` under `header` and the rest empty.
    fn one_region(header: &[&str], rows: &[&[&str]]) -> String {
        page(&[("region-1", header, rows), ("region-2", header, &[]), ("region-3", header, &[]), ("region-4", header, &[])])
    }

    fn error(page: &str) -> String {
        format!("{:#}", parse(page.as_bytes()).unwrap_err())
    }

    #[test]
    fn a_full_row_parses_every_column() {
        let html = one_region(&FULL, &[&["  Salem 1  ", "0", "2016-04-14", "REFUELING &amp; MAINTENANCE\r\nSEE EN #1", "*", "1"]]);

        let StatusPage::Report(rows) = parse(html.as_bytes()).unwrap() else { panic!("no rows") };

        assert_eq!(
            rows,
            [Row {
                region: 1,
                unit: "Salem 1".to_owned(),
                power: 0,
                detail: Some(Detail {
                    down: Some(NaiveDate::from_ymd_opt(2016, 4, 14).unwrap()),
                    reason: Some("REFUELING & MAINTENANCE\nSEE EN #1".to_owned()),
                    changed: true,
                    scrams: 1,
                }),
            }]
        );
    }

    #[test]
    fn a_nul_alone_in_a_reason_cell_reads_as_blank() {
        let html = one_region(&FULL, &[&["Ginna", "100", "", "\0", "", ""]]);

        let StatusPage::Report(rows) = parse(html.as_bytes()).unwrap() else { panic!("no rows") };

        assert_eq!(rows[0].detail.as_ref().unwrap().reason, None);
    }

    #[test]
    fn the_template_is_enforced() {
        let cases = [
            (one_region(&["Unit", "Power", "Reason"], &[]), "unknown header"),
            (one_region(&FULL, &[&["Ginna", "100", "", ""]]), "4 cells under a 6-column header"),
            (one_region(&HELD, &[&["", "100"]]), "no unit name"),
            (one_region(&HELD, &[&["Ginna", "101"]]), "power 101 is over 100"),
            (one_region(&HELD, &[&["Ginna", "full"]]), "power \"full\""),
            (one_region(&FULL, &[&["Ginna", "0", "02/20/1996", "", "", ""]]), "down date \"02/20/1996\""),
            (one_region(&FULL, &[&["Ginna", "100", "", "", "x", ""]]), "change mark \"x\""),
            (one_region(&FULL, &[&["Ginna", "100", "", "", "", "#"]]), "scram count \"#\""),
            (page(&[("region-1", &HELD, &[])]), "expected 4 region tables, found 1"),
            (
                page(&[("region-2", &HELD, &[]), ("region-1", &HELD, &[]), ("region-3", &HELD, &[]), ("region-4", &HELD, &[])]),
                "expected region-1 next",
            ),
            (page(&[]), "neither region tables nor"),
            ("<html><body><table></table></body></html>".to_owned(), "expected exactly one"),
        ];
        for (html, expected) in cases {
            let error = error(&html);
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }

    #[test]
    fn errors_name_the_region_row_and_unit() {
        let html = one_region(&FULL, &[&["Ginna", "100", "", "", "", ""], &["Salem 1", "0", "yesterday", "", "", ""]]);

        assert_eq!(error(&html), "region 1: row 2: unit \"Salem 1\": down date \"yesterday\": input contains invalid characters");
    }

    #[test]
    fn a_page_that_is_not_utf8_fails() {
        assert!(parse(b"\xff\xfe").unwrap_err().to_string().contains("not UTF-8"));
    }
}
