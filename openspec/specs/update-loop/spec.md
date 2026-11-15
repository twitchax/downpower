# update-loop Specification

## Purpose
The update loop runs inside `downpower serve`: at start and, with `DOWNPOWER_FETCH=daily`, every six hours, it fetches what the NRC and EIA have published since, refreshes the ledger, and records the run for `/status`.

## Requirements
### Requirement: When the loop runs
`downpower serve` SHALL start one update task that runs once at start. When `DOWNPOWER_FETCH` is exactly `daily` each run SHALL fetch before refreshing, and the next run SHALL start six hours after the previous one ends; with any other value or none, the task SHALL run once without fetching, only refreshing the ledger. (PRD-0002 T-011)

#### Scenario: Fetching off
- **WHEN** the task starts with no sources over a fixture raw store
- **THEN** it records exactly one run, which fetched nothing and names the build it made current
- **AND** the task ends

#### Scenario: Only `daily` turns fetching on
- **WHEN** `DOWNPOWER_FETCH` is unset, `yes`, or `daily`
- **THEN** only `daily` gives the loop sources to fetch from

### Requirement: Status pages a run fetches
A fetching run SHALL fetch, through one fetcher paced at the nrc.gov rate as `raw-store` describes: every unsettled status page from 60 days before the hold's edge (today − 28 days) through the edge; each date after the edge and before yesterday that the store has no record for; and yesterday's and today's pages. (PRD-0002 T-011)

#### Scenario: A first run over an empty store
- **WHEN** a run fetches from a fake nrc.gov into an empty store
- **THEN** it stores 89 status pages: 61 through the hold's edge, the 26 between it and yesterday, then yesterday and today

#### Scenario: A second run the same day
- **WHEN** the same day's next run fetches
- **THEN** it requests only yesterday's and today's status pages

### Requirement: Event Notification pages and NUC_STATUS a run fetches
A fetching run SHALL also fetch the unsettled Event Notification pages dated from 7 days ago through today, and a fresh `NUC_STATUS`, checked as `raw-store` describes, when none is stored or the newest was fetched 7 or more days ago. (PRD-0002 T-011)

#### Scenario: NUC_STATUS once a week
- **WHEN** a second run follows a first that fetched `NUC_STATUS` the same day
- **THEN** the second run fetches no `NUC_STATUS`
- **AND** the store holds one

### Requirement: A failed fetch still refreshes
A fetch error SHALL end the run's fetching, land in the run's `errors`, and leave the run to refresh the ledger from what the store holds. A refresh error SHALL land in `errors` too, and neither SHALL stop the loop. (PRD-0002 T-011)

#### Scenario: nrc.gov answers 403
- **WHEN** a run's first request draws a 403
- **THEN** the run records one error mentioning 403
- **AND** it still makes a build current from the stored pages

### Requirement: Each run refreshes the ledger
After fetching, each run SHALL refresh the ledger as `ledger` describes, so it rebuilds or updates only when the raw store's content or the binary's derived version changed and quarantines what the catalog doesn't know. One task fetches and refreshes, so two builds never run at once. (PRD-0002 T-011 T-019)

#### Scenario: Nothing new
- **WHEN** a run's refetches find every page unchanged
- **THEN** the run rebuilds nothing

#### Scenario: An unknown tag on the daily path
- **WHEN** a stored page carries `Zorbulating the Flux Capacitor` and the loop runs
- **THEN** the run records no error and makes a build current
- **AND** the build's `unknown_tags` counts `zorbulating the flux capacitor` once per use

### Requirement: Run records
Each run SHALL be recorded with its start and finish times; the status and Event Notification fetch counts (`stored`, `unchanged`, `missing`, `skipped`), null when the run didn't fetch; the last day of a `NUC_STATUS` it fetched; the build table it rebuilt or updated, null when the ledger was current; and its errors. (PRD-0002 T-011 T-015 T-019)

#### Scenario: A recorded run
- **WHEN** a fetching run that rebuilt is recorded
- **THEN** the newest runs read back as exactly that run, naming the current build's table
