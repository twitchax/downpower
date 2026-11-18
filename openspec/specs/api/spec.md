# api Specification

## Purpose
The HTTP surface of `downpower serve` beyond the rendered pages: the `/api/*` routes the views read, their wire formats, ETags and 503s, the health check, gzip, and the cache policy every response carries.

## Requirements
### Requirement: Routes
The server SHALL answer `GET /healthz` with 200 and the body `ok`, and SHALL serve `/api/barcode/{layer}`, `/api/lost/{layer}`, `/api/unit-day/{docket}/{date}`, `/api/events`, `/api/events/counts`, `/api/events/{number}`, `/api/markers` and `/api/status`, where `{layer}` is `attributed` or `observed` and any other layer answers 404. (PRD-0001 T-001, PRD-0002 T-013 T-014 T-015, PRD-0004 T-003 T-006)

#### Scenario: Health
- **WHEN** a client GETs `/healthz`
- **THEN** it answers 200 `ok`

#### Scenario: A layer that doesn't exist
- **WHEN** a client GETs `/api/barcode/bogus` or `/api/lost/bogus`
- **THEN** each answers 404

### Requirement: 503 until a build exists
The barcode, lost and unit-day routes SHALL answer 503 with `Retry-After: 30` and the body `the ledger is building` while no build has been made current, and the barcode and lost routes SHALL do the same when the serving build stored no buffer or series for the layer. (PRD-0002 T-013 T-016)

#### Scenario: A fresh volume
- **WHEN** the database has never held a build
- **THEN** `/api/barcode/attributed` and `/api/unit-day/05000323/2025-07-15` answer 503

### Requirement: Routes read the serving build
The data routes SHALL read the build the current-build record names, also while that record is in a shape the running binary can't parse, as `ledger` describes. (PRD-0002 T-020)

#### Scenario: A record in an older shape
- **WHEN** the current record has lost its summary
- **THEN** `/api/barcode/attributed`, `/api/lost/attributed` and `/api/unit-day/05000323/2025-07-15` still answer 200

### Requirement: Barcode buffer format
`/api/barcode/{layer}` SHALL answer `application/octet-stream`: the magic `DPB1`; the first day as days from the common era (i32), the day count (u32) and the row count (u16), little-endian; per row its docket then its unit name, each a length byte and UTF-8; then one cell byte per row per day, row-major. The rows SHALL be every catalog unit, in docket order. (PRD-0002 T-013)

#### Scenario: The fixture build's buffer
- **WHEN** the ledger is built from the 2025-07-15 fixture page and the buffer fetched
- **THEN** it decodes with one row per catalog unit, dockets ascending
- **AND** Diablo Canyon 2's cell on 2025-07-15 is its unit-day's attributed cell

### Requirement: Barcode span
The buffer SHALL run from the build's first row date through its last, every day present. (PRD-0002 T-013)

#### Scenario: One page and EIA's next day
- **WHEN** `downpower serve` builds from the 2025-07-15 page plus a `NUC_STATUS` ending 07-16
- **THEN** the buffer starts on 2025-07-15 and spans 2 days

### Requirement: Barcode cells
A cell byte SHALL hold the family's index in its high 3 bits (Refueling 0, Fuel cycle 1, Forced 2, Maintenance 3, Equipment 4, Environmental 5, Grid 6, Unattributed 7) and the power bucket ⌊30 × power / 100⌋ in its low 5 bits, 100% alone in bucket 30. `0xFF` SHALL mark a day with no row: the unit was out of service or nothing reported it. (PRD-0002 T-013)

#### Scenario: Every cell round-trips
- **WHEN** each family is packed with each bucket from 0 to 30
- **THEN** every byte unpacks to its family and bucket, and none is `0xFF`

#### Scenario: Buckets cover every power once
- **WHEN** the powers 0 to 100 are bucketed
- **THEN** each falls in exactly one bucket, 99% in bucket 29 and 100% in bucket 30

### Requirement: Barcode ETag and revalidation
The barcode SHALL carry a weak ETag, `W/"<first 16 hex digits of the buffer's sha256>"`, so unchanged data keeps it and different data moves it, whether a full rebuild or an in-place update wrote the buffer, and SHALL answer 304 with the ETag when `If-None-Match` lists it, weak or strong. (PRD-0002 T-013 T-021)

#### Scenario: Revalidating across a rebuild
- **WHEN** a client revalidates with the ETag it was given, before and after a rebuild of the same raw store
- **THEN** both answers are 304

#### Scenario: New data
- **WHEN** a page for 2025-07-16 is added and the ledger rebuilt
- **THEN** the barcode answers 200 with a different ETag

#### Scenario: New data updated in place
- **WHEN** a running server has served the barcode, and a page for 2025-07-16 then updates the serving table in place
- **THEN** the barcode answers 200 with a different ETag
- **AND** the old ETag no longer revalidates

### Requirement: Monthly series
`/api/lost/{layer}` SHALL answer JSON `{"first": <first day of the first month>, "lost_kwh": [<8 kWh totals>, ...]}`, one entry per month the build covers, zeros included, each indexed by family index and summing every row's `lost_kwh` under the family the layer gives it, so the months add up to the build's lost energy. (PRD-0002 T-014)

#### Scenario: The fixture build
- **WHEN** `/api/lost/attributed` is fetched after a rebuild
- **THEN** its kWh sum to the build summary's `lost_kwh`

#### Scenario: The JSON shape
- **WHEN** a series holds 9 kWh of Fuel cycle in January 2024
- **THEN** it serializes as `{"first":"2024-01-01","lost_kwh":[[0,9,0,0,0,0,0,0]]}`

### Requirement: One unit-day
`/api/unit-day/{docket}/{date}` SHALL answer the serving build's row for that docket and `YYYY-MM-DD` date as JSON, 404 when the build has no such row, and 400 when the date doesn't parse. (PRD-0002 T-013)

#### Scenario: A row, a missing day and a bad date
- **WHEN** a client asks for 05000323 on 2025-07-15, on 1990-01-01, and on `July`
- **THEN** the answers are 200 with that row, 404, and 400

### Requirement: Unit-day JSON
A unit-day SHALL serialize with `docket`, `date`, `power`, `source`, `eia_power`, `reason`, `down`, `changed`, `scrams`, `causes` (each `{cause, tag}`), `flags`, `transition`, `xrefs`, `unclassified`, `unknown`, `cause`, `family` (snake_case), `method` (the names `ledger` lists), `evidence`, `capacity_kw` and `lost_kwh`. (PRD-0002 T-006 T-008 T-009 T-013, PRD-0003 T-005)

#### Scenario: Evidence by kind
- **WHEN** evidence is a tag, an inherited day, or none
- **THEN** it serializes as `{"tag": "refueling outage"}`, `{"inherited_from": "2025-07-01"}`, or `"none"`

#### Scenario: Evidence from Event Notifications
- **WHEN** evidence is a resolved event or references that don't resolve
- **THEN** it serializes as `{"event": {"number": 45286, "decided_by": {"scram": "manual_reactor"}}}`, with `{"section": "50.72(b)(1)(i)(A)"}` or null for `decided_by`, or as `{"xrefs": [{"number": 49154, "reason": "retracted"}, {"number": null, "reason": "no_number"}]}`

#### Scenario: Methods by name
- **WHEN** each method serializes
- **THEN** it reads `precedence_v1`, `inherit_14d`, `xref_en_v1`, `xref_unresolved`, `no_cause`, `reason_pending`, `no_report` or `unknown_tag`, and parses back

### Requirement: Status JSON
`/api/status` SHALL answer JSON `{"build": ..., "serving": ..., "runs": [...]}`: the current build with its `version`, `table`, `built_at`, `raw_digest`, the summary `ledger` lists and its event archive (`pages`, `empty`, `events`, `categories`, `failed`, `unknown_facilities`), or null when none is readable; the table the views read, or null; and the newest 20 runs, newest first, as `update-loop` records them. (PRD-0002 T-015 T-021, PRD-0003 T-010)

#### Scenario: After one run
- **WHEN** one recorded run has built the fixture ledger
- **THEN** `/api/status` parses as the `/status` page reads it, with a build whose dates count each covered date once and one error-free run that rebuilt

#### Scenario: A build record in an older shape
- **WHEN** the serving build's record can't be read by this binary
- **THEN** `build` is null and `serving` names the table the views still read

### Requirement: Gzip
Responses SHALL be gzip-compressed when the request accepts gzip. (PRD-0002 T-013)

#### Scenario: The observed barcode
- **WHEN** a client GETs `/api/barcode/observed` with `Accept-Encoding: gzip`
- **THEN** the response carries `Content-Encoding: gzip`

### Requirement: The event list
`/api/events` SHALL answer JSON `{total, page, pages, rows}`: page `page`, 50 to a page, of the serving build's events the query selects (dates, category groups, state, a name, a unit's docket, a 10 CFR prefix, emergency class, scram, and words all found in the title, text or names), newest first by date then number, each row with its number, date, group, place, state, units, title, 10 CFR codes and scram and retraction flags. (PRD-0004 T-002 T-003)

#### Scenario: Two filters
- **WHEN** a client GETs `/api/events?state=AL&scram=1` over the fixture events
- **THEN** it lists EN 45290 alone, of a total of 1

#### Scenario: Stable pages
- **WHEN** the 38 fixture events are read 10 to a page
- **THEN** the four pages hold every event once, in the order one page of 50 holds them

### Requirement: Event counts
`/api/events/counts` SHALL answer JSON `{first, months}`: the events the same filters select, counted per month from the first's month to the last's in eight series, the seven largest category groups and Other, so the months sum to the list's total. (PRD-0004 T-003)

#### Scenario: Small groups fold into Other
- **WHEN** the fixture's three Hospital events and its Transportation Event are counted
- **THEN** all four count in the eighth series

### Requirement: One event
`/api/events/{number}` SHALL answer the serving build's event `number` as stored (the event as its page shows it, its identity and its page's date), with `nrc`, its anchor on that NRC page, `label`, its category as published, and `unit_names`; 404 for a number it doesn't hold, and 400 for one that isn't digits. (PRD-0004 T-003 T-005)

#### Scenario: An unknown number
- **WHEN** a client GETs `/api/events/99999` or `/api/events/zebra`
- **THEN** they answer 404 and 400

### Requirement: Event markers buffer
`/api/markers` SHALL answer `application/octet-stream`: `DPM1`, a table of dockets, and one marker per unit of each mapped, unretracted reactor event in the serving build (a docket index, its day from 1999-01-01, a scram flag and the event number), under a weak `ETag` from the bytes, 304 to a request holding it. (PRD-0004 T-006)

#### Scenario: The fixture's reactor events
- **WHEN** the build holds the eight fixture event pages
- **THEN** the buffer holds 15 markers, EN 45290 on Browns Ferry 3 on 2009-08-24 a scram, and retracted EN 35269 none

### Requirement: Event tags
The list and the counts SHALL carry a weak `ETag` from the serving build's table and `built_at` and the query, and SHALL answer 304 to a request whose `If-None-Match` holds it. An update in place moves `built_at`, so it moves the tag. (PRD-0004 T-003)

#### Scenario: Revalidating a list
- **WHEN** a client GETs a list again with the `ETag` it was given
- **THEN** it answers 304

### Requirement: Cache policy
Every response under `/pkg/`, the content-hashed site assets, SHALL carry `Cache-Control: public, max-age=31536000, immutable`, and every other response (pages, `/healthz`, `/api/*` and unhashed public files such as `/robots.txt`) SHALL carry `Cache-Control: no-cache`. (PRD-0001 T-001)

#### Scenario: A hashed asset
- **WHEN** the policy is asked about `/pkg/app.0123abcd.wasm`
- **THEN** it answers `public, max-age=31536000, immutable`

#### Scenario: Health and the barcode revalidate
- **WHEN** a client GETs `/healthz` or `/api/barcode/attributed`
- **THEN** the response carries `Cache-Control: no-cache`
