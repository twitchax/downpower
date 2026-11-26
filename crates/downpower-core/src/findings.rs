//! Findings (DESIGN.md Decisions 25 and 44–47): short notes in `findings/NNNN-slug.md`, each arguing
//! one thing a figure shows. The app's build script parses them with [`render`] and compiles the
//! table in, so the server and the browser share it and neither parses markdown.

#[cfg(feature = "render")]
pub mod render;

use chrono::NaiveDate;

/// A finding as the app carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finding {
    pub number: u16,
    /// The file's name without `.md`, `NNNN-slug`, which is also its path under `/findings`.
    pub stem: &'static str,
    pub title: &'static str,
    /// One sentence, for the index and the link preview.
    pub summary: &'static str,
    /// The view's path (`/trends`), its query (`?from=1999-01-01`, or empty at the defaults) and
    /// the id of the figure the note is about.
    pub view: &'static str,
    pub query: &'static str,
    pub figure: &'static str,
    /// The day the note was written, and the data's last day then: its numbers hold as of these.
    pub written: NaiveDate,
    pub through: NaiveDate,
    /// The note, rendered.
    pub html: &'static str,
}

impl Finding {
    /// The finding's page.
    #[must_use]
    pub fn href(&self) -> String {
        format!("/findings/{}", self.stem)
    }

    /// The whole view at the finding's state, scrolled to its figure.
    #[must_use]
    pub fn view_href(&self) -> String {
        format!("{}{}#{}", self.view, self.query, self.figure)
    }
}

/// A date in the compiled table. The build script has already checked it, so the fallback is
/// unreachable; it keeps this a `const fn` without a panic.
#[must_use]
pub const fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    match NaiveDate::from_ymd_opt(year, month, day) {
        Some(date) => date,
        None => NaiveDate::MIN,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_finding_links_its_page_and_its_view_at_the_figure() {
        let finding = Finding {
            number: 2,
            stem: "0002-scrams-fell",
            title: "Scrams fell",
            summary: "They did.",
            view: "/trends",
            query: "?from=1999-01-01",
            figure: "scram-rate",
            written: date(2026, 11, 26),
            through: date(2026, 11, 25),
            html: "<p>They did.</p>\n",
        };
        assert_eq!(finding.href(), "/findings/0002-scrams-fell");
        assert_eq!(finding.view_href(), "/trends?from=1999-01-01#scram-rate");
        assert_eq!(finding.written, NaiveDate::from_ymd_opt(2026, 11, 26).unwrap_or_default());
    }
}
