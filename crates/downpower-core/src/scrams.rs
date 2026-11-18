//! The scram checks' counts (DESIGN.md Decision 33, PRD-0003 T-007 and T-008), as a build records
//! them and `/status` reads them. `downpower-ingest`'s `scrams` module runs the internal check and
//! its `scram_dataset` module the reconciliation against the NRC's scram dataset.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// EN scrams by what the status reports show.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCounts {
    pub dropped: u64,
    pub named: u64,
    pub no_report: u64,
    pub low_power: u64,
    pub missed: u64,
}

impl EventCounts {
    /// The EN scrams the reports show.
    #[must_use]
    pub const fn agreed(&self) -> u64 {
        self.dropped + self.named
    }

    /// The EN scrams the reports could show: the unit is listed, and it scrammed from enough power
    /// for a drop to show.
    #[must_use]
    pub const fn checked(&self) -> u64 {
        self.agreed() + self.missed
    }
}

/// Report scrams by what the Event Notifications hold.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCounts {
    pub event: u64,
    pub retracted: u64,
    pub no_scram_code: u64,
    pub missed: u64,
}

impl ReportCounts {
    /// The report scrams an event reports.
    #[must_use]
    pub const fn agreed(&self) -> u64 {
        self.event
    }

    /// The report scrams but those only a retracted event matches.
    #[must_use]
    pub const fn checked(&self) -> u64 {
        self.event + self.no_scram_code + self.missed
    }
}

/// The NRC's scram dataset against our EN scrams, by (event number, docket), over the span from
/// the first of the dataset's first year through its last date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetCounts {
    pub first: NaiveDate,
    pub last: NaiveDate,
    /// Scrams both list.
    pub agree: u64,
    /// Scrams only the dataset lists.
    pub dataset_only: u64,
    /// Scrams only our Event Notifications hold.
    pub ours_only: u64,
    /// Known differences: scrams one side holds retracted (the dataset's `NRC_retract_flag`, or our
    /// page's banner), and ours from a reactor that wasn't critical, which the dataset mostly leaves out.
    pub known: u64,
}

impl DatasetCounts {
    /// The scrams both list.
    #[must_use]
    pub const fn agreed(&self) -> u64 {
        self.agree
    }

    /// The scrams either side lists, but the known differences.
    #[must_use]
    pub const fn checked(&self) -> u64 {
        self.agree + self.dataset_only + self.ours_only
    }
}

/// Both directions' counts, and the dataset's when one is stored, as a build's summary records them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub events: EventCounts,
    pub reports: ReportCounts,
    #[serde(default)]
    pub dataset: Option<DatasetCounts>,
}
