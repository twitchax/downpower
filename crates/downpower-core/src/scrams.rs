//! The internal scram check's counts (DESIGN.md Decision 33, PRD-0003 T-007), as a build records
//! them and `/status` reads them. `downpower-ingest`'s `scrams` module runs the check.

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

/// Both directions' counts, as a build's summary records them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub events: EventCounts,
    pub reports: ReportCounts,
}
