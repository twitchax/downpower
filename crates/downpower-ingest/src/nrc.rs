//! The NRC's daily page series, at their Drupal addresses (live since about Sep 2026; the old
//! `/reading-rm/...html` URLs redirect here). See `docs/research/data-sources.md`.

use chrono::{Datelike as _, Days, NaiveDate, Weekday};

use crate::raw::{Record, Series};

/// The live site.
pub const BASE_URL: &str = "https://www.nrc.gov";

/// The first report date of both series.
pub const FIRST_DAY: NaiveDate = match NaiveDate::from_ymd_opt(1999, 1, 1) {
    Some(date) => date,
    None => panic!("1999-01-01 is a valid date"),
};

const COLLECTIONS: &str = "/documents-reports/document-collections/events-reports-associated-with";

impl Series {
    /// The path of the page for `date`, e.g. `.../power-reactor-status-reports/1999/19990105ps`.
    #[must_use]
    pub fn path(self, date: NaiveDate) -> String {
        let (dir, suffix) = match self {
            Self::PowerStatus => ("power-reactor-status-reports", "ps"),
            Self::EventNotifications => ("event-notification-reports", "en"),
        };
        format!("{COLLECTIONS}/{dir}/{}/{}{suffix}", date.year(), date.format("%Y%m%d"))
    }

    /// Whether the NRC publishes a page for `date`. Event Notification pages skip weekends; federal
    /// holidays still get one (the 2026-11 backfill drew no 404s).
    #[must_use]
    pub fn publishes_on(self, date: NaiveDate) -> bool {
        match self {
            Self::PowerStatus => true,
            Self::EventNotifications => !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
        }
    }

    /// How long after its date a page may still change. A status page shows only `Unit | Power`
    /// until it is 28 days old, when the reason column fills in.
    #[must_use]
    pub const fn hold(self) -> Days {
        match self {
            Self::PowerStatus => Days::new(28),
            Self::EventNotifications => Days::new(2),
        }
    }

    /// The dates in `from..=to` that have a page, oldest first.
    pub fn dates(self, from: NaiveDate, to: NaiveDate) -> impl Iterator<Item = NaiveDate> {
        from.iter_days().take_while(move |date| *date <= to).filter(move |date| self.publishes_on(*date))
    }
}

/// Whether `record` settles its page for good: a 200 or a 404 captured once the page's hold had
/// passed. A rerun skips settled pages and fetches the rest again.
#[must_use]
pub fn settles(record: &Record) -> bool {
    matches!(record.status, 200 | 404) && record.date.checked_add_days(record.series.hold()).is_some_and(|due| record.captured_at.date_naive() >= due)
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::raw::{Hash, Source};

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    fn fetched(series: Series, date: &str, status: u16, at: &str) -> Record {
        let at = format!("{at}T12:00:00Z").parse().unwrap();
        Record {
            source: Source::Nrc,
            series,
            date: day(date),
            url: String::new(),
            status,
            captured_at: at,
            fetched_at: at,
            sha256: (status == 200).then(|| Hash::of(b"page")),
            len: None,
        }
    }

    #[test]
    fn paths_follow_the_drupal_scheme() {
        assert_eq!(
            Series::PowerStatus.path(day("1999-01-05")),
            "/documents-reports/document-collections/events-reports-associated-with/power-reactor-status-reports/1999/19990105ps"
        );
        assert_eq!(
            Series::EventNotifications.path(day("2024-07-02")),
            "/documents-reports/document-collections/events-reports-associated-with/event-notification-reports/2024/20240702en"
        );
    }

    #[test]
    fn event_pages_skip_weekends_and_status_pages_do_not() {
        // 1999-01-01 is a Friday.
        let status: Vec<_> = Series::PowerStatus.dates(day("1999-01-01"), day("1999-01-04")).collect();
        let events: Vec<_> = Series::EventNotifications.dates(day("1999-01-01"), day("1999-01-04")).collect();

        assert_eq!(status.len(), 4);
        assert_eq!(events, vec![day("1999-01-01"), day("1999-01-04")]);
        assert_eq!(Series::PowerStatus.dates(FIRST_DAY, day("1998-12-31")).count(), 0);
    }

    #[test]
    fn a_status_page_settles_only_once_its_reasons_are_published() {
        assert!(!settles(&fetched(Series::PowerStatus, "2026-10-14", 200, "2026-11-10")));
        assert!(settles(&fetched(Series::PowerStatus, "2026-10-14", 200, "2026-11-11")));
        assert!(settles(&fetched(Series::PowerStatus, "1999-01-01", 404, "2026-11-12")));
        assert!(!settles(&fetched(Series::PowerStatus, "2026-11-11", 404, "2026-11-12")));
        assert!(!settles(&fetched(Series::PowerStatus, "1999-01-01", 500, "2026-11-12")));
    }

    #[test]
    fn an_event_page_settles_after_two_days() {
        assert!(!settles(&fetched(Series::EventNotifications, "2026-11-11", 200, "2026-11-12")));
        assert!(settles(&fetched(Series::EventNotifications, "2026-11-10", 200, "2026-11-12")));
    }
}
