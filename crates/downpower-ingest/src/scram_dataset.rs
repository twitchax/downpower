//! The NRC's scram dataset (DESIGN.md Decision 33, PRD-0003 T-008): `scram-event-notifications.xlsx`,
//! every reactor scram since 2011 with its Event Notification number. It is a cross-check, never a
//! source: our EN scrams are reconciled against it by (event number, docket) over its span, and the
//! disagreements are counted and listed.
//!
//! The NRC last modified it 2025-05-13 (728 rows, 2011-01-19 to 2025-05-12), so it isn't kept
//! current; a run fetches it weekly in case that changes.

use std::collections::BTreeMap;
use std::io::Cursor;

use anyhow::{Context as _, bail, ensure};
use calamine::{Data, Reader as _, Xlsx};
use chrono::{Datelike as _, Days, NaiveDate};
use serde::{Deserialize, Serialize};

pub use downpower_core::scrams::DatasetCounts;

use crate::Res;
use crate::events::{EventSet, Scram};
use crate::facilities::Identity;

/// Where nrc.gov serves it, under `nrc::BASE_URL`.
pub const PATH: &str = "/cdn/legacy/reactors/operating/dataset/scram-event-notifications.xlsx";

/// The sheet holding the rows; the other totals them by year.
const SHEET: &str = "Public Site Data";

/// The header, trimmed. A new column set fails the parse, the way a new one fails the status parser.
const COLUMNS: [&str; 11] = [
    "Year",
    "Scram #",
    "Plant Name",
    "Docket",
    "Site",
    "PWR/BWR",
    "Auto/Manual",
    "Date",
    "Event_Notification_Number",
    "NRC_retract_flag",
    "Link",
];

/// Day zero of Excel's serial dates, which counts 1900 as a leap year.
const EXCEL_EPOCH: NaiveDate = match NaiveDate::from_ymd_opt(1899, 12, 30) {
    Some(date) => date,
    None => panic!("1899-12-30 is a valid date"),
};

/// One scram, one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub event: u32,
    /// As the catalog writes it, e.g. `05000530`, or `05200025` for Vogtle 3.
    pub docket: String,
    pub date: NaiveDate,
    /// The Auto/Manual column: `A` or `M`.
    pub automatic: bool,
    /// `NRC_retract_flag`: the NRC withdrew the event but still lists it.
    pub retracted: bool,
}

/// Parses the workbook's rows, skipping the empty ones it is padded with.
///
/// # Errors
///
/// When the bytes aren't an xlsx, the sheet or its header has changed, or a row doesn't parse.
pub fn parse(bytes: &[u8]) -> Res<Vec<Row>> {
    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).context("reading the scram dataset's workbook")?;
    let range = workbook.worksheet_range(SHEET).with_context(|| format!("the scram dataset's {SHEET:?} sheet"))?;
    let mut rows = range.rows();
    let header: Vec<String> = rows.next().context("the scram dataset has no header")?.iter().map(|cell| cell.to_string().trim().to_owned()).collect();
    ensure!(header == COLUMNS, "the scram dataset's columns changed: {header:?}");
    rows.enumerate()
        .filter(|(_, cells)| cells.iter().any(|cell| *cell != Data::Empty))
        .map(|(index, cells)| row(cells).with_context(|| format!("scram dataset row {}", index + 2)))
        .collect()
}

fn row(cells: &[Data]) -> Res<Row> {
    let docket = match cells.get(3) {
        Some(Data::String(docket)) if docket.len() == 8 && docket.bytes().all(|byte| byte.is_ascii_digit()) => docket.clone(),
        other => bail!("Docket {other:?} is not eight digits"),
    };
    let automatic = match cells.get(6) {
        Some(Data::String(kind)) if kind == "A" => true,
        Some(Data::String(kind)) if kind == "M" => false,
        other => bail!("Auto/Manual {other:?} is neither A nor M"),
    };
    let serial = whole(cells.get(7)).context("Date")?;
    let date = EXCEL_EPOCH.checked_add_days(Days::new(serial.into())).context("Date is out of range")?;
    let event = whole(cells.get(8)).context("Event_Notification_Number")?;
    let retracted = match whole(cells.get(9)).context("NRC_retract_flag")? {
        0 => false,
        1 => true,
        flag => bail!("NRC_retract_flag {flag} is neither 0 nor 1"),
    };
    Ok(Row {
        event,
        docket,
        date,
        automatic,
        retracted,
    })
}

/// A cell holding a whole number, as Excel stores numbers and dates.
fn whole(cell: Option<&Data>) -> Res<u32> {
    let value = match cell {
        Some(Data::Int(value)) => return u32::try_from(*value).with_context(|| format!("{value} is out of range")),
        Some(Data::Float(value)) => *value,
        Some(Data::DateTime(value)) => value.as_f64(),
        other => bail!("{other:?} is not a number"),
    };
    ensure!(value.fract() == 0.0, "{value} is not whole");
    format!("{value:.0}").parse().with_context(|| format!("{value} is out of range"))
}

/// How a scram fares between the dataset and our Event Notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agreement {
    /// Both list it.
    Agree,
    /// Only the dataset: we hold no such event, or its line for the unit has SCRAM Code `N`.
    DatasetOnly,
    /// Only our Event Notifications.
    OursOnly,
    /// The dataset's `NRC_retract_flag` is set: a known difference.
    NrcRetracted,
    /// The dataset lists it, and our page carries the retraction banner: a known difference.
    WeRetracted,
    /// Only ours, from a reactor that wasn't critical: a known difference. The dataset lists 7 such
    /// scrams against 701 from a critical reactor (PRD-0003.log.md#t-008).
    NotCritical,
}

/// A scram either side lists, by event number and docket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub event: u32,
    pub docket: String,
    pub date: NaiveDate,
    pub outcome: Agreement,
}

/// The reconciliation over the dataset's span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciled {
    /// The first of the dataset's first year: it counts scrams by calendar year.
    pub first: NaiveDate,
    /// The dataset's last date.
    pub last: NaiveDate,
    /// In (event number, docket) order.
    pub cases: Vec<Case>,
}

impl Reconciled {
    #[must_use]
    pub fn counts(&self) -> DatasetCounts {
        let mut counts = DatasetCounts {
            first: self.first,
            last: self.last,
            agree: 0,
            dataset_only: 0,
            ours_only: 0,
            known: 0,
        };
        for case in &self.cases {
            *match case.outcome {
                Agreement::Agree => &mut counts.agree,
                Agreement::DatasetOnly => &mut counts.dataset_only,
                Agreement::OursOnly => &mut counts.ours_only,
                Agreement::NrcRetracted | Agreement::WeRetracted | Agreement::NotCritical => &mut counts.known,
            } += 1;
        }
        counts
    }
}

/// Reconciles `rows` with the EN scrams in `events` (a unit's SCRAM Code other than `N` on an event
/// not retracted) dated within the dataset's span. `None` when there are no rows.
#[must_use]
pub fn reconcile(rows: &[Row], events: &EventSet) -> Option<Reconciled> {
    let first = rows.iter().map(|row| row.date).min()?.with_ordinal(1)?;
    let last = rows.iter().map(|row| row.date).max()?;
    // Each scram's date, and whether the reactor was critical.
    let mut ours: BTreeMap<(u32, &str), (NaiveDate, bool)> = BTreeMap::new();
    for (&number, event) in &events.events {
        let date = event.occurred.date;
        if event.retracted || !(first..=last).contains(&date) {
            continue;
        }
        let Some(Identity::Reactor { dockets }) = events.identities.get(&number) else { continue };
        for (unit, docket) in event.units.iter().zip(dockets) {
            if unit.scram.is_some_and(|scram| scram != Scram::No) {
                ours.insert((number, docket.as_str()), (date, unit.critical));
            }
        }
    }
    let mut cases: Vec<Case> = rows
        .iter()
        .map(|row| {
            let outcome = if row.retracted {
                Agreement::NrcRetracted
            } else if events.events.get(&row.event).is_some_and(|event| event.retracted) {
                Agreement::WeRetracted
            } else if ours.remove(&(row.event, row.docket.as_str())).is_some() {
                Agreement::Agree
            } else {
                Agreement::DatasetOnly
            };
            Case {
                event: row.event,
                docket: row.docket.clone(),
                date: row.date,
                outcome,
            }
        })
        .collect();
    // A row the NRC flagged retracted may still match a live scram of ours, which it accounts for.
    for row in rows.iter().filter(|row| row.retracted) {
        ours.remove(&(row.event, row.docket.as_str()));
    }
    cases.extend(ours.into_iter().map(|((event, docket), (date, critical))| Case {
        event,
        docket: docket.to_owned(),
        date,
        outcome: if critical { Agreement::OursOnly } else { Agreement::NotCritical },
    }));
    cases.sort_by(|a, b| (a.event, &a.docket).cmp(&(b.event, &b.docket)));
    Some(Reconciled { first, last, cases })
}
