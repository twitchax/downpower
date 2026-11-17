# raw-store Specification

## Purpose
The raw store is downpower's system of record: every NRC status and Event Notification page and every EIA `NUC_STATUS` file it fetches, kept compressed under its sha256 beside an append-only manifest, fetched politely, and read back by every ledger build.

## Requirements
### Requirement: Content-addressed blobs
The system SHALL keep the raw store under `<data dir>/raw`, each fetched body zstd-compressed at `blobs/<first two hex digits>/<sha256>.zst`, named by the lowercase hex sha256 of its uncompressed bytes. Storing bytes already present SHALL leave the existing blob as it is and return the same hash. (PRD-0002 T-001)

#### Scenario: The same page stored twice
- **WHEN** the bytes of one status page are stored twice
- **THEN** both stores return the page's sha256
- **AND** the store holds exactly one blob, `blobs/<hh>/<sha256>.zst`, which reads back as the page

### Requirement: Manifest records
The system SHALL append one JSON line per fetch to `<data dir>/raw/manifest.jsonl`, holding `source` (`nrc` or `eia`), `series` (`power_status`, `event_notifications` or `nuc_status`), the report `date`, `url`, the final HTTP `status`, `captured_at`, `fetched_at`, `sha256` (null for a 404) and the uncompressed `len`. A `NUC_STATUS` record's `date` SHALL be the day it was fetched. (PRD-0002 T-001 T-007)

#### Scenario: A 404 is on record without a body
- **WHEN** the page for 1999-01-03 answers 404
- **THEN** the manifest gains a line for 1999-01-03 with status 404 and a null `sha256`

### Requirement: Checked reads and a self-repairing manifest
Reading a blob SHALL fail when it is missing, does not decompress, or does not hash to its name, and a manifest hash that is not 64 lowercase hex digits SHALL be rejected. Opening the store SHALL truncate a final manifest line cut off mid-append back to the last complete line. (PRD-0002 T-001)

#### Scenario: A tampered blob
- **WHEN** a blob's file is replaced by other compressed bytes
- **THEN** reading it fails with "does not match its hash"

#### Scenario: A torn last line
- **WHEN** the manifest ends in half a JSON line and the store is opened again
- **THEN** the half line is gone
- **AND** the next appended record follows the last complete one

### Requirement: The newest capture wins
For each series and report date, a build SHALL read the newest manifest record, so a later fetch of a date supersedes earlier ones. (PRD-0002 T-001 T-008)

#### Scenario: A held page, then its settled copy
- **WHEN** the manifest holds a numbers-only capture of a date and later a capture with reasons
- **THEN** the date's newest record is the capture with reasons

### Requirement: Content digest
The raw store SHALL expose a digest over the newest record's hash for every series and date, which SHALL move when a page arrives or a date's content changes and SHALL NOT move when a fetch records a hash the date already has. (PRD-0002 T-011 T-019)

#### Scenario: A refetch of the same bytes
- **WHEN** a date's page is recorded again with the same hash
- **THEN** the digest is unchanged
- **AND** recording a different hash for the date moves it

### Requirement: NRC page addresses
The fetcher SHALL request NRC pages from `https://www.nrc.gov` at `/documents-reports/document-collections/events-reports-associated-with/<dir>/<yyyy>/<yyyymmdd><suffix>`, with `power-reactor-status-reports` and suffix `ps` for a status page and `event-notification-reports` and suffix `en` for an Event Notification page. Both series start on 1999-01-01. (PRD-0002 T-002)

#### Scenario: A 1999 status page
- **WHEN** the status page for 1999-01-05 is fetched
- **THEN** the request goes to `.../power-reactor-status-reports/1999/19990105ps`

### Requirement: Publishing days
The fetcher SHALL request a status page and an Event Notification page for every calendar day, weekends and federal holidays included, oldest first, since an Event Notification page holds the events of its own day. (PRD-0002 T-002, PRD-0003 T-006)

#### Scenario: A weekend
- **WHEN** both series are fetched from Friday 1999-01-01 to Monday 1999-01-04
- **THEN** four status pages and four Event Notification pages are requested

### Requirement: Declared User-Agent and request pacing
Every request to nrc.gov and eia.gov SHALL carry the User-Agent `downpower/<major>.<minor> (+https://github.com/twitchax/downpower)`, today `downpower/0.1 (+https://github.com/twitchax/downpower)`. One fetcher SHALL start NRC requests at least its interval apart, retries included, and the interval against nrc.gov SHALL never be under 5 seconds (as `cli` enforces). (PRD-0002 T-002 T-007)

#### Scenario: A run against a fake nrc.gov
- **WHEN** a fetcher with a 100 ms interval fetches four days, two of them twice
- **THEN** every request carries the project User-Agent
- **AND** no two request starts are closer than the interval

### Requirement: Retries and backoff
A 429, a 5xx, a network error, or a 200 body whose last 4 KiB lacks `</html>` SHALL be retried after a backoff that doubles up to a cap, waiting at least a `Retry-After` given in seconds, and a page that fails every attempt SHALL stop the run with an error. The nrc.gov values (1 minute doubling to 30, 8 attempts) are tuning; `crates/downpower-ingest/src/fetch.rs` is the authority. (PRD-0002 T-002)

#### Scenario: Rate limited once, then cut off once
- **WHEN** 1999-01-02 answers 429 once and 1999-01-04 arrives truncated once
- **THEN** each of those dates is requested twice and stored once

#### Scenario: A page that never recovers
- **WHEN** a page answers 503 on every one of a fetcher's 3 attempts
- **THEN** the run fails with "failed 3 times"
- **AND** the page was requested exactly 3 times

### Requirement: A 403 stops the run
A 403 SHALL stop the run at once with an error naming the URL and the 403, without a retry and without a manifest record. Any status other than 200, 404, 429 and 5xx SHALL stop the run as well. (PRD-0002 T-002)

#### Scenario: The bot block returns
- **WHEN** nrc.gov answers 403 to the first request of a four-day run
- **THEN** the run fails with an error containing 403
- **AND** only that one request was sent, and the manifest is unchanged

### Requirement: Fetch counts
A fetch run SHALL report how many pages it `stored`, how many it found `unchanged`, how many were `missing` (a 404, recorded with no hash), and how many it `skipped` as settled by an earlier run. (PRD-0002 T-002 T-019)

#### Scenario: A first run and its rerun
- **WHEN** a run over 1999-01-01 to 01-04 meets three pages and one 404, then runs again
- **THEN** the first run reports 3 stored and 1 missing
- **AND** the rerun reports 4 skipped and sends no request

### Requirement: Holds and settled pages
A page SHALL count as settled once a 200 or 404 for it was captured on or after its date plus its 28-day hold: a status page's reason column fills in then, and an Event Notification page's events are still being edited until then. A run SHALL skip settled pages and fetch the rest again, so an interrupted run resumes where it stopped. (PRD-0002 T-002, PRD-0003 T-006)

#### Scenario: A status page at its hold's edge
- **WHEN** the 2026-10-14 status page was captured on 2026-11-10
- **THEN** it is not settled
- **AND** a capture on 2026-11-11 settles it

#### Scenario: An Event Notification page
- **WHEN** the 2026-10-15 Event Notification page was captured on 2026-11-11
- **THEN** it is not settled
- **AND** a capture on 2026-11-12 settles it

### Requirement: Unchanged refetches keep the stored copy
When a refetched 200 page matches the newest stored copy of its date once every `<script>` naming `BOOMR` (the CDN's beacon) is dropped from both, the fetcher SHALL record the fetch against the stored copy's hash, store no new blob and count it `unchanged`, so the digest holds. Any other difference SHALL be stored as new content. (PRD-0002 T-019)

#### Scenario: Pages inside their hold, fetched three times
- **WHEN** two held pages are fetched, fetched again with only a new beacon, then once more after one report changes
- **THEN** the runs count 2 stored, then 2 unchanged, then 1 stored and 1 unchanged
- **AND** the digest holds through the second run and moves on the third

### Requirement: NUC_STATUS fetch
Fetching `NUC_STATUS` SHALL download `https://www.eia.gov/opendata/bulk/NUC_STATUS.zip` in one unpaced request and store the one file inside the zip rather than the zip. A non-200 response, a body that is not a zip, a zip holding other than exactly one file, or a file over 512 MiB SHALL fail with nothing recorded. (PRD-0002 T-007)

#### Scenario: The bulk file
- **WHEN** EIA serves a zip holding `NUC_STATUS.txt`
- **THEN** the store holds that file's bytes under their sha256, with an `eia` / `nuc_status` record of status 200

#### Scenario: A maintenance page instead of a zip
- **WHEN** EIA answers 404, or 200 with HTML, or a zip with two files
- **THEN** the fetch fails and the manifest stays empty

### Requirement: A stored NUC_STATUS is checked
After storing a `NUC_STATUS`, `downpower fetch`, `downpower backfill` and the update loop SHALL parse the stored copy and fail when it doesn't parse or names a generator `catalog/units.csv` lacks, with EIA's body already on record. (PRD-0002 T-007 T-011)

#### Scenario: EIA adds a generator
- **WHEN** a fetched `NUC_STATUS` holds a generator the units catalog doesn't list
- **THEN** the command fails, listing the generator
- **AND** the file is still in the raw store
