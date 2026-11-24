# ledger Specification

## Purpose
The ledger holds one row per in-service unit-day from the first stored status page on, built from the raw store and the committed `catalog/` tables: power, the observed reason tags, one attributed cause with its method and evidence, and lost output. It also decides when a build is rebuilt, updated in place, or left alone, and keeps the previous build serving meanwhile.

## Requirements
### Requirement: Status page template
The parser SHALL read a status page as exactly one `div.nrc-power-reactor-status-report-day` holding four tables, `region-1` to `region-4` in order, each headed `Unit | Power | Down | Reason or Comment | Change in Report (*) | Scrams (#)` or, inside the 28-day hold, `Unit | Power`. Any departure SHALL fail the parse, naming the region and row, and the unit for a cell that doesn't parse. (PRD-0002 T-003)

#### Scenario: A malformed date in a row
- **WHEN** region 1's second row, Salem 1, gives `yesterday` as its Down date
- **THEN** the parse fails with `region 1: row 2: unit "Salem 1": down date "yesterday"`

#### Scenario: A page inside the hold
- **WHEN** the 2026-11-12 page is parsed while it shows only `Unit | Power`
- **THEN** its 95 rows carry power and no other column

### Requirement: Status page cells
Power SHALL be an integer from 0 to 100, Down an ISO date or blank, the change mark `*` or blank, and Scrams a count with a blank read as 0. Cell text SHALL be trimmed, its entities decoded and its CRLFs made LFs, and a blank reason cell, a lone NUL included, SHALL read as no reason. (PRD-0002 T-003)

#### Scenario: Every column of a settled row
- **WHEN** a row reads `Salem 1 | 0 | 2016-04-14 | REFUELING &amp; MAINTENANCE<CRLF>SEE EN #1 | * | 1`
- **THEN** it parses to power 0, down 2016-04-14, the reason `REFUELING & MAINTENANCE\nSEE EN #1`, a change mark and 1 scram

#### Scenario: A NUL in a blank reason
- **WHEN** a 2025-era row's reason cell holds only a NUL
- **THEN** the row has no reason

### Requirement: Days with no report
A report block with no region tables that reads `No data available for this date.` SHALL parse as a day the NRC published no report for, and a block with neither SHALL fail. (PRD-0002 T-003)

#### Scenario: A permanent gap
- **WHEN** the stored page for 1999-05-08 is parsed
- **THEN** it reads as no data

### Requirement: Units keyed by docket
Every unit-day SHALL be keyed by NRC docket (`05000xxx`, or `05200xxx` for Part 52 units) through `catalog/units.csv`, which maps each docket to its display name, every spelling the status pages have used (`;`-separated), its Event Notification site names and unit number, and one EIA plant and generator. A malformed or non-reactor docket, or a docket, page spelling or generator listed twice, SHALL fail the catalog. (PRD-0002 T-004, PRD-0003 T-004)

#### Scenario: Two spellings of one unit
- **WHEN** the catalog lists `LaSalle 1;La Salle 1` for docket 05000373 and EIA plant 6026 generator 1
- **THEN** both spellings and the generator find docket 05000373
- **AND** an unlisted name such as `Zion 1` finds none

### Requirement: Unit states
`catalog/units.csv` SHALL give each unit's state, the one its Event Notifications name, and a state that isn't two capital letters SHALL fail the catalog. (PRD-0005 T-005)

#### Scenario: A state spelled out
- **WHEN** a catalog row gives the state `Ill`
- **THEN** the catalog fails naming `state "Ill"`

### Requirement: Units in service
A unit SHALL be in service from its `commercial` date when it has one, through its `shutdown` date, and again from its `restart` date, and a day it is out of service SHALL have no ledger row, whatever its Power column reads. A date without a `source`, a shutdown before commercial operation, or a restart without an earlier shutdown SHALL fail the catalog. `catalog/units.csv` is the authority for the dates. (PRD-0002 T-004 T-009)

#### Scenario: A shutdown and a restart
- **WHEN** a unit shuts down on 2022-05-20 and restarts on 2025-10-01
- **THEN** it is in service on 2022-05-20 and 2025-10-01, and out of service on 2022-05-21 and 2025-09-30

#### Scenario: Browns Ferry 1
- **WHEN** the committed catalog is read
- **THEN** Browns Ferry 1 is out of service on 1999-01-01 and 2007-05-21, and in service from 2007-05-22

#### Scenario: A row after a shutdown
- **WHEN** Palisades appears on the 2022-05-21 page, the day after its shutdown
- **THEN** the day has no Palisades row

### Requirement: Unknown unit names and generators
Under the fail policy (`downpower rebuild`, `downpower backfill`, `cargo xtask summary`), a page's unit name or a newest-`NUC_STATUS` generator the catalog lacks SHALL fail the build, naming it. Under the quarantine policy (the server's own refreshes) a row with an unknown name SHALL be skipped and the name counted with its uses in the summary's `unknown_units`. (PRD-0002 T-004 T-008 T-011)

#### Scenario: Mystery 9
- **WHEN** a page lists `Mystery 9` beside Diablo Canyon 2
- **THEN** the fail policy fails with `unknown unit "Mystery 9"`
- **AND** the quarantine policy builds Diablo Canyon 2's row alone and counts `Mystery 9` once

### Requirement: Reason tags
A reason SHALL split into tags on `;` and line breaks, each normalized to lowercase with whitespace runs collapsed to one space and its ends trimmed, and blank tags dropped. (PRD-0002 T-005)

#### Scenario: A two-tag reason
- **WHEN** the reason is `Steam Generator Limitations;\n\nMaximum  Generation Alert\n`
- **THEN** its tags are `steam generator limitations` and `maximum generation alert`

### Requirement: Tag classification
A tag SHALL be known by, in order, its exact key in `catalog/tags.toml`, the first `[[rules]]` regex that matches it, or its listing in `catalog/unclassified.tsv`, which accepts it on purpose as Unattributed. A key or rule labels a tag `cause:<fine cause>`, `flag:<flag>`, `transition` or `xref`, and no other matching applies. (PRD-0002 T-005)

#### Scenario: A key beats a rule
- **WHEN** `scram time testing` is classified
- **THEN** its exact key matches, a Maintenance cause, though a rule for scrams would catch it too

#### Scenario: The first rule wins
- **WHEN** a tag matches two rules
- **THEN** the earlier rule in `catalog/tags.toml` labels it

### Requirement: A scram tag names a trip
The committed table SHALL label `scram` a tag naming a reactor, manual or automatic trip or a scram, and SHALL give its own cause to a tripped pump, line or breaker, a turbine or generator trip, a test of a trip system or of scram times, and work on scram valves, the scram discharge volume or trip solenoids. `catalog/tags.toml` is the authority for the spellings and its keys for the exceptions. (PRD-0003 T-007)

#### Scenario: A pump trip and a trip test
- **WHEN** `one recirc pump tripped`, `turbine trip testing in progress` and `isolating leaking scram outlet valve` are classified
- **THEN** they are `recirculation_pump`, `testing` and `maintenance`
- **AND** `rx scram during turbine trip testing. see en#45265` is `scram`

#### Scenario: A turbine trip
- **WHEN** `turbine trip due to ehc rupture` and `automatic reactor shutdown due to main turbine trip - see en #45789.` are classified
- **THEN** they are `turbine_generator` and `scram`

### Requirement: Tag table checks
The tag table SHALL fail to load when a label names an undefined cause or flag, a key or listed tag isn't normalized or appears twice, a flag appears twice, a pattern doesn't compile, `no_lost_output` names an unknown cause, or `catalog/unclassified.tsv` doesn't start with its `tag` header. (PRD-0002 T-005 T-009)

#### Scenario: A key in capitals
- **WHEN** `catalog/tags.toml` keys `Refueling Outage`
- **THEN** the table fails with `key "Refueling Outage" is not normalized`

### Requirement: The reasons snapshot guards the table
`catalog/reasons.tsv` SHALL list every distinct tag on the stored status pages with its uses and its uses below 100% power, and the test suite, so CI, SHALL fail when a tag in it is unknown or when `catalog/unclassified.tsv` lists a tag a key or rule already matches. (PRD-0002 T-005)

#### Scenario: The committed snapshot
- **WHEN** `cargo make test` runs
- **THEN** every tag in `catalog/reasons.tsv` classifies
- **AND** no listed tag is one a key or rule matches

### Requirement: Families and fine causes
Each fine cause SHALL belong to one of eight families: Refueling, Fuel cycle, Forced, Maintenance, Equipment, Environmental, Grid and Unattributed. `catalog/tags.toml` is the authority for the fine causes, and "environmental conditions" SHALL be `environmental_unspecified`, never heat. (PRD-0002 T-005 T-013)

#### Scenario: Sample classifications
- **WHEN** the committed table classifies `coastdown to refueling outage`, `reactor trip due to main transformer fire. see en# 52210.` and `reduced power due to environmental conditions`
- **THEN** they are `coastdown` (Fuel cycle), `scram` (Forced) and `environmental_unspecified` (Environmental)
- **AND** `hot weather alert` is a flag, `defueled` a transition and `see en# 55049` an xref to 55049

### Requirement: The observed layer
For each unit-day with reason text, the ledger SHALL record what the tags say: each cause a tag names with that tag, in reason order; the flags, each once; whether a tag named a transition; EN references with their numbers, null where a tag carries none; tags accepted as Unattributed; and tags nothing knows. (PRD-0002 T-006)

#### Scenario: Flags alone
- **WHEN** a reason reads `Hot Weather Alert; Conservative Operations; Hot Weather Alert`
- **THEN** its flags are `hot_weather_alert` and `conservative_ops`
- **AND** it names no cause, so it is attributed `no_cause`

#### Scenario: Accepted and unknown tags
- **WHEN** a reason reads `.; Zorbulating the flux capacitor; Refueling Outage`
- **THEN** `.` is an accepted tag, `zorbulating the flux capacitor` an unknown one, and one cause is observed

### Requirement: Attribution by precedence
A day whose tags name a cause, or whose EN references resolve, SHALL be attributed the cause of the highest family in the order Forced > Refueling > Maintenance > Equipment > Environmental > Grid > Fuel cycle > Unattributed. A tie SHALL go to a cause the tags name before a resolved one, and then to the one named first. The method is `precedence_v1` with the naming tag as evidence, or `xref_en_v1` with the resolved event. (PRD-0002 T-006, PRD-0003 T-005)

#### Scenario: Equipment over Fuel cycle
- **WHEN** a reason reads `Coastdown to Refueling Outage;\r\n\r\nTransformer Limitation`
- **THEN** the day is attributed `transformer` by `precedence_v1` with the tag `transformer limitation` as evidence

#### Scenario: A resolved scram over maintenance
- **WHEN** Crystal River 3 reads `Reduced Power for Maintenance; See EN #45286` the day after EN 45286, its `M/R` scram
- **THEN** the day is attributed `scram` by `xref_en_v1`

#### Scenario: A tie goes to the reason
- **WHEN** Browns Ferry 3 reads `Forced Outage; See EN #45290`, an `M/R` scram on that unit
- **THEN** the day is attributed `forced_outage` by `precedence_v1`

### Requirement: Inherited causes
A day with no cause but a transition SHALL take the unit's last attributed cause when that day is at most 14 calendar days back, with method `inherit_14d` and evidence `inherited_from` the day the cause was observed. Inherited days extend the chain, a newly observed cause restarts it, and a day with no cause neither extends nor breaks it; with nothing in reach the day is Unattributed, still `inherit_14d`. (PRD-0002 T-006)

#### Scenario: Bridging back
- **WHEN** a unit reads `Refueling Outage` on day 1, `Startup in Progress` on day 3, `Increasing Power` on day 17 and `Holding Power` on day 32
- **THEN** days 3 and 17 inherit `refueling_outage` from day 1
- **AND** day 32 is Unattributed with no evidence

#### Scenario: A month of DEFUELED
- **WHEN** a unit reads `REFUELING OUTAGE` on day 1 and `DEFUELED` on days 2 to 31
- **THEN** every one of days 2 to 31 inherits `refueling_outage` from day 1

#### Scenario: A new cause restarts the chain
- **WHEN** a unit reads `Refueling Outage`, `Increasing Power`, `Forced Outage`, `Startup in Progress` on days 1 to 4
- **THEN** day 4 inherits `forced_outage` from day 3

#### Scenario: A resolved reference starts a chain
- **WHEN** Crystal River 3 reads `See EN #45286` on 2009-08-25 and `Startup in Progress` on 08-28
- **THEN** 08-28 inherits `scram` from 08-25

### Requirement: Why a day is Unattributed
A day with neither a cause nor a transition SHALL be Unattributed with method `xref_unresolved` and each reference's number and reason as evidence when it carries EN references none of which resolve, and `no_cause` otherwise. A row inside the 28-day hold SHALL be `reason_pending`, a row from EIA alone `no_report`, and a quarantined row `unknown_tag`. (PRD-0002 T-006 T-008, PRD-0003 T-005)

#### Scenario: An unresolved Event Notification
- **WHEN** a reason reads `SEE EN #49154` and the event set has no EN 49154
- **THEN** the day is Unattributed by `xref_unresolved` with evidence `[{number: 49154, reason: missing}]`

#### Scenario: A held row
- **WHEN** a page inside the hold reports Diablo Canyon 2 at 55%
- **THEN** its row is Unattributed by `reason_pending`, with no reason

### Requirement: EN reference resolution
A day's `See EN# N` SHALL resolve when the event set holds event N, its unit table lists the day's unit, and it isn't retracted; otherwise it SHALL stay unresolved with the reason `no_number`, `missing`, `other_unit` or `retracted`, checked in that order. A resolved reference's cause SHALL come from `catalog/xref.toml` over the unit's SCRAM Code and the event's 10 CFR sections, with the event number and the deciding field as evidence. (PRD-0003 T-005)

#### Scenario: Three ways to resolve
- **WHEN** Crystal River 3 cites EN 45286 (SCRAM Code `M/R`), Catawba 2 cites EN 35268 (`N`, 50.72(b)(1)(i)(A) `PLANT S/D REQD BY TS`), and FitzPatrick cites EN 35278 (`N`, 50.72(b)(2)(iii)(D))
- **THEN** they resolve to `scram` decided by the scram code, `required_shutdown` decided by the section, and `en_reported` decided by nothing

#### Scenario: References that don't resolve
- **WHEN** Browns Ferry 2 cites the retracted EN 35269, and on another day EN 45290, a Browns Ferry 3 scram; Catawba 2 cites EN 35275, a fuel cycle facility's; Cooper cites EN 99999; and FitzPatrick cites `SEE EN # PENDING`
- **THEN** their reasons are `retracted`, `other_unit`, `other_unit`, `missing` and `no_number`

### Requirement: The xref rule table
`catalog/xref.toml` SHALL hold ordered rules and a fallback, each naming a fine cause from `catalog/tags.toml`, and a rule SHALL match on exactly one of SCRAM Codes and 10 CFR sections. A section rule's code SHALL match a section's code or a prefix of it that ends a paragraph, spaces ignored, and its description, when given, SHALL match ignoring case and spacing. The first matching rule decides; with none, the fallback. (PRD-0003 T-005)

#### Scenario: The committed rules
- **WHEN** a resolved unit's SCRAM Code is `A/R`, `M/R`, `A` or `M`, or the event lists 50.72(b)(2)(i) or (b)(1)(i)(A) as `PLANT S/D REQD BY TS`
- **THEN** the cause is `scram`, then `required_shutdown`, and otherwise `en_reported`, all Forced

#### Scenario: One code, two meanings
- **WHEN** an event lists 50.72(b)(2)(i) as `DEG/UNANALYZED COND`, its meaning before the 2000 revision of 10 CFR 50.72
- **THEN** no rule matches it, and `50.7` would not match `50.72(b)` either

#### Scenario: A table the catalog can't back
- **WHEN** a rule names a cause `catalog/tags.toml` lacks, matches on both SCRAM Codes and sections, or lists an unknown SCRAM Code
- **THEN** loading the table fails, naming the rule

### Requirement: Unknown tags
A reason carrying a tag nothing knows SHALL fail the build under the fail policy, naming the date, unit and tag. Under the quarantine policy the row SHALL be Unattributed with method `unknown_tag` and the unknown tags as evidence, keep its known tags observed, and add each unknown tag's uses to the summary's `unknown_tags`. (PRD-0002 T-005 T-008 T-011)

#### Scenario: A tag no one has seen
- **WHEN** Diablo Canyon 2's reason reads `Zorbulating the flux capacitor; Refueling Outage`
- **THEN** the fail policy fails naming `zorbulating the flux capacitor`
- **AND** the quarantine policy builds an `unknown_tag` row that still observes the refueling cause, and counts the tag once

### Requirement: EIA's daily numbers
A build SHALL read the newest stored `NUC_STATUS`'s generator series, skipping plant and US totals, as each generator's daily net capacity in MW and its power, 100 minus the percent outage. A percent outage that isn't a whole number from 0 to 100, a negative capacity, a series listed twice, or a generator whose capacity and percent outage cover different days SHALL fail the build. (PRD-0002 T-007)

#### Scenario: Power from percent outage
- **WHEN** Diablo Canyon 2's percent outage on 2025-07-05 is 45
- **THEN** its EIA power that day is 55
- **AND** on 2025-07-15 its EIA power equals the NRC page's

### Requirement: Where a row's power comes from
A row for a unit the day's page reports SHALL take the page's power, `source` `nrc_page`, with `eia_power` holding EIA's value that day where it has one. An in-service unit the day's page lacks (no page, a no-data page, or not listed) that EIA reports SHALL get a row with EIA's power, `source` `eia`, no reason, and method `no_report`. (PRD-0002 T-007 T-008)

#### Scenario: A no-data day
- **WHEN** the page for a day reads no data and EIA reports Diablo Canyon 2 at 55%
- **THEN** the day's only row is Diablo Canyon 2 at 55% from `eia`, `no_report`
- **AND** a day with no stored page gives the same row

### Requirement: Build span and order
A build SHALL produce one row per in-service unit-day, ordered by docket within each date, from the first date the raw store has a status-page record for (a 404 included) through the later of the last such date and the last day `NUC_STATUS` covers. A raw store with no status-page record SHALL build no rows. (PRD-0002 T-008)

#### Scenario: One page and EIA's next day
- **WHEN** the store holds the 2025-07-15 page, a 404 for 07-16 and a `NUC_STATUS` ending 07-16 with only Diablo Canyon 2
- **THEN** the build covers 2025-07-15 to 07-16, every listed unit on the 15th and Diablo Canyon 2 from `eia` on the 16th

### Requirement: Units listed twice
When a page lists a unit more than once, copies that differ only in the change mark SHALL merge into one row that keeps any mark. Otherwise a copy identical, spelling aside, to the unit's row on the previous published page SHALL drop out, and a day where that leaves other than one distinct report SHALL fail the build under either policy. (PRD-0002 T-003 T-007 T-008)

#### Scenario: A change mark on one copy
- **WHEN** the 2006-06-22 page lists Wolf Creek 1 twice, one copy marked changed
- **THEN** the day has one Wolf Creek 1 row, marked changed

#### Scenario: A stale repeat in either order
- **WHEN** a page lists La Salle 1 twice and one copy repeats the unit's row on the previous page
- **THEN** the other copy is the row, whichever comes first

#### Scenario: Copies no rule separates
- **WHEN** a page lists Perry 1 at 100% and at 96%, and neither repeats the previous page
- **THEN** the build fails naming Perry 1

### Requirement: Lost output
Each row SHALL carry `capacity_kw`, net capacity in whole kW (EIA-860 net summer capacity from `catalog/capacity.csv` for 1999 to 2006, else the latest `NUC_STATUS` rating on or before the day), and `lost_kwh`, (100 − power)% × capacity × 24 h rounded down: zero when the attributed cause is in `no_lost_output`, and null without a capacity. (PRD-0002 T-009)

#### Scenario: A 45-point reduction
- **WHEN** Diablo Canyon 2, rated 1,118 MW by `NUC_STATUS`, reads 55% on 2025-07-05
- **THEN** its row carries `capacity_kw` 1118000 and `lost_kwh` 12074400

#### Scenario: Full electrical output
- **WHEN** Diablo Canyon 2 reads 98% with `100% Electrical Capability`
- **THEN** the row is `full_electrical_output` (Equipment) with `lost_kwh` 0

#### Scenario: No NUC_STATUS
- **WHEN** a build has no `NUC_STATUS` and a 2025 row is below 100%
- **THEN** the row has no capacity and no lost output, and counts as a reduced row without capacity

### Requirement: NUC_STATUS ratings carry forward
A `NUC_STATUS` rating SHALL carry across days EIA skips and past the end of the file, and a zero SHALL NOT count as a rating. (PRD-0002 T-007 T-009)

#### Scenario: A rating zeroed before retirement
- **WHEN** EIA rates a generator at zero for weeks while it still runs
- **THEN** its capacity on those days is its last positive rating

### Requirement: Pre-2007 capacity table
`catalog/capacity.csv` (`docket,year,net_summer_mw,eia860_year,status`) SHALL fail to load on a year outside 1999 to 2006, a rating taken from an earlier file than its year, a rating not above 0 MW and below 10,000 MW, or a unit-year listed twice, and SHALL rate every unit in service during each of those years. (PRD-0002 T-009)

#### Scenario: A 2007 row
- **WHEN** the table holds a row for 2007
- **THEN** it fails to load with "outside 1999–2006"

#### Scenario: Every pre-2007 unit-year rated
- **WHEN** the committed table is checked against the units catalog
- **THEN** no unit in service in a year from 1999 to 2006 lacks a rating for it

### Requirement: Build summary
Every build SHALL record a summary: rows; first and last day; rows from EIA alone; rows inside the hold; NRC rows EIA also reports and how many disagree on power; units a page lists twice; quarantined tags and unit names with their uses; total lost kWh; rows below 100% without a capacity; each covered date counted once as `reasons`, `held`, `eia` or `missing`; the scram check's counts; and the scram dataset's, when one is stored. (PRD-0002 T-008 T-015, PRD-0003 T-007 T-008)

#### Scenario: Dates by source
- **WHEN** a build meets a page with reasons, a held page, a no-data day EIA fills, and a day with nothing
- **THEN** its dates count 1 each as `reasons`, `held`, `eia` and `missing`

### Requirement: EN scrams in the status reports
Each unit a live event's table gives a SCRAM Code other than `N` SHALL count once: `dropped` when an NRC row the day of the event or the next shows it 10 or more points below its initial power; else `named` when such a row has the Scrams (#) count, a `scram` tag, or cites the event; else `no_report` when neither day has an NRC row; else `low_power` from under 10%; else `missed`. (PRD-0003 T-007)

#### Scenario: A scram the reports show
- **WHEN** EN 45286 scrams Crystal River 3 from 100% on 2009-08-24 and it reads 100% that day and 0% the next
- **THEN** it counts as `dropped`

#### Scenario: A quick restart the report names
- **WHEN** Browns Ferry 3, scrammed by EN 45290 from 100%, reads 95% the next day citing EN 45290
- **THEN** it counts as `named`

#### Scenario: A retracted event
- **WHEN** the event is retracted
- **THEN** its scram counts toward nothing

### Requirement: Status-report scrams in the event reports
A unit's report scram SHALL start on the first of a run of consecutive NRC rows with the Scrams (#) count or a `scram` tag, dated by its Down date when that is within the 7 days before, else the row's date. It SHALL count once: `event` when a live event on the unit within 2 days, or cited anywhere in the reason, gives a SCRAM Code other than `N`; else `retracted` when a retracted one does; else `no_scram_code` when events are within 2 days; else `missed`. (PRD-0003 T-007)

#### Scenario: A scram tag citing its event
- **WHEN** Crystal River 3 first reads `Reactor Trip - See EN #45286` nine days after EN 45286, one tag labelled `scram`
- **THEN** the report scram counts as `event` through EN 45286

#### Scenario: Two days marked, one scram
- **WHEN** Crystal River 3 reads `Automatic Reactor Trip` on 2009-08-25 and 08-26
- **THEN** one report scram counts, as `event`, through EN 45286

#### Scenario: The event says no scram
- **WHEN** Peach Bottom 2 reads `Manual Reactor Scram` on 2009-08-25 and its only event near then, EN 45289, says `N`
- **THEN** it counts as `no_scram_code`

#### Scenario: Only a retracted event
- **WHEN** the one event matching a report scram is retracted
- **THEN** it counts as `retracted`, a known difference outside the agreement rate

### Requirement: The scram dataset reconciled
Each build SHALL reconcile the newest stored scram dataset by event number and docket with the EN scrams of live events from the first of its first year through its last date: `agree` when both list one; a known difference when the dataset flags it retracted, our event is retracted, or only ours holds it from a reactor not critical; else dataset only or ours only. A dataset that doesn't parse SHALL fail a rebuild under the fail policy and be skipped under quarantine. (PRD-0003 T-008)

#### Scenario: Each class
- **WHEN** the dataset lists EN 45286 at Crystal River 3, EN 45289 at Peach Bottom 2 (whose line says `N`), and EN 45300 flagged retracted, but not Browns Ferry 3's EN 45290
- **THEN** 45286 agrees, 45289 is dataset only, 45290 ours only, and 45300 a known difference

#### Scenario: A scram while shut down
- **WHEN** the dataset lacks EN 45290 and Browns Ferry 3 wasn't critical
- **THEN** 45290 is a known difference, outside the agreement rate

#### Scenario: New columns
- **WHEN** the dataset's `Docket` column is renamed
- **THEN** parsing fails, naming the header it found

### Requirement: The event set
Every build SHALL hold one event per number found on the newest capture of each stored event page, taken from the page fetched last (between pages fetched together, the later date), and SHALL record the pages read, those reading "No events found", the events by category label, each event's page date, and the dates of pages that failed to parse. A page that fails SHALL fail a rebuild under the fail policy and be skipped and counted under quarantine. (PRD-0003 T-003 T-010, PRD-0004 T-005)

#### Scenario: An event the NRC edited
- **WHEN** EN 54960 is on a page fetched 2026-11-13 and, edited, on a page fetched 2026-11-14
- **THEN** the build holds the edited copy, once

#### Scenario: An event page that doesn't parse
- **WHEN** the stored page for 2020-10-21 isn't an event page
- **THEN** a refresh counts 2020-10-21 as failed and keeps the other events
- **AND** `downpower rebuild` fails naming the event page for 2020-10-21

### Requirement: Reactor and facility identity
A Power Reactor event's units SHALL map to dockets by site name and unit number through `catalog/units.csv` and `catalog/facilities.csv`, a Fuel Cycle Facility event by state and name, and a Research or Non-Power Reactor event by license number, whatever its Docket field says. Names SHALL match ignoring case and runs of spaces. (PRD-0003 T-004)

#### Scenario: One site, two catalogs
- **WHEN** an event names `DRESDEN` units 1 and 2
- **THEN** it maps to 05000010 from `facilities.csv` and 05000237 from `units.csv`, in that order

#### Scenario: One name, two facilities
- **WHEN** Fuel Cycle events name `GENERAL ELECTRIC CO.` in IL and in CA
- **THEN** they map to GE Morris (07200001) and GE Vallecitos (07000754)

#### Scenario: A docket printed a digit short
- **WHEN** a Non-Power Reactor event prints license R-79 under docket `0500123`
- **THEN** it maps to 05000123, Missouri S&T

### Requirement: Licensee groups
Every other event SHALL group by its state and its license number's letters and digits in upper case, under the licensee name on the group's highest-numbered event that has one. A Part 21 event, or one with no state or no digit in its license, SHALL stay ungrouped. (PRD-0003 T-004)

#### Scenario: One license, three spellings
- **WHEN** IL events 10, 30 and 40 carry license `12-0072206`, `120072206` and `12-00722-06`, named `U S ARMY`, `DEPARTMENT OF ARMY` and nothing
- **THEN** all three form one group shown as `DEPARTMENT OF ARMY`
- **AND** the same license in IN is another group

#### Scenario: Placeholders
- **WHEN** an Agreement State event's license reads `General License` or `N/A`
- **THEN** the event stays ungrouped

### Requirement: Unknown facilities
Under the fail policy, an event whose reactor site and unit, fuel cycle facility or non-power reactor license the catalogs lack SHALL fail the build, naming the event and the facility. Under quarantine the event SHALL be stored without an identity and the facility counted with its events in the build's `unknown_facilities`. (PRD-0003 T-004)

#### Scenario: A renamed fuel cycle facility
- **WHEN** EN 54962 names `Framatome Anp Kennewick` in WA
- **THEN** `downpower rebuild` fails with `event 54962: unknown Fuel Cycle Facility "Framatome Anp Kennewick" in WA`
- **AND** a refresh stores EN 54962 without an identity and counts that facility once

### Requirement: The facilities catalog
`catalog/facilities.csv` SHALL list each facility once by docket, with a kind (`power_reactor` with a reactor docket, EN site names and unit; `fuel_cycle` with EN names; `non_power_reactor` with a reactor docket and license), a two-letter state and a source. A row missing what its kind needs, or a site and unit, state and name, or license listed twice across both catalogs, SHALL fail the catalog. (PRD-0003 T-004)

#### Scenario: A site claimed twice
- **WHEN** `facilities.csv` lists `PALO VERDE` unit 1, which `units.csv` already lists
- **THEN** the catalog fails naming `Reactor "PALO VERDE" unit 1`

### Requirement: Event units
Every build SHALL store, beside each event, its identity, and for each unit of a mapped reactor event one `event_unit_<n>` row keyed `[number, unit]` with the unit's docket, the day the event happened, the unit's scram code and whether the event is retracted. (PRD-0003 T-004)

#### Scenario: Two units scrammed in one event
- **WHEN** EN 45285 lists Quad Cities units 1 and 2
- **THEN** the build holds its rows `[45285, 1]` with docket 05000254 and `[45285, 2]` with 05000265

### Requirement: Refueling outages
A build SHALL count a refueling outage for each run of a unit's consecutive days at 0% power with a day at power on each side, so a run cut by the archive's edges, a shutdown or the present doesn't count, when Refueling is the most common attributed family of its days, ties included. Its length SHALL be its days at 0%, and it SHALL start on its first. (PRD-0006 T-002)

#### Scenario: The fixture's status pages
- **WHEN** the trends are collected over the fixture's August 2009 status pages
- **THEN** they hold one outage, Crystal River 3 (05000302) from 2009-08-24 for 3 days

### Requirement: Trend scrams
A build's trends SHALL count an unretracted reactor event's catalog unit as an automatic scram when its SCRAM Code is `A/R` or `A` and a manual one when `M/R` or `M`, in the month the event occurred. (PRD-0006 T-002)

#### Scenario: A scram retracted
- **WHEN** the fixture's EN 45290 is coded `A/R` and EN 45286 is retracted
- **THEN** the month counts one automatic scram and no manual one

### Requirement: Fitness-for-duty events
An unretracted event SHALL be a fitness-for-duty event when a 10 CFR section's code, spaces ignored, is 26.73 or 26.719 or one of their paragraphs, or is 26.417(b)(1), or when `catalog/ffd.toml` includes it; and not when the catalog excludes it. A title alone SHALL make no event one. (PRD-0005 T-001)

#### Scenario: Sections and paragraphs
- **WHEN** events are coded `26.719 (b)(2)`, `26.417(b)(2)`, `26.7190` and `73.71(b)(1)`
- **THEN** only the first is a fitness-for-duty event, under 26.719

#### Scenario: The reviewed lists
- **WHEN** the catalog includes an event with no FFD section and excludes one coded 26.719
- **THEN** the first is a fitness-for-duty event and the second isn't

### Requirement: Fitness-for-duty tags
Each fitness-for-duty event SHALL carry a substance, role, kind and test from the first matching phrase rule of each in `catalog/ffd.toml`, read from its title and text lowercased with whitespace collapsed and the catalog's `strip` patterns removed, or `none`. A named drug SHALL outrank alcohol. `catalog/ffd.toml` is the authority for the phrases. (PRD-0005 T-002)

#### Scenario: A random positive
- **WHEN** the text reads "A contract supervisor had a confirmed positive for marijuana during a random fitness-for-duty test."
- **THEN** the tags are marijuana, supervisor, positive and random

#### Scenario: The known misfires
- **WHEN** a text says "THC LICENSEE", a tritium sample "positive", isopropyl alcohol, the Bureau of Alcohol, Tobacco, Firearms and Explosives, or contraband weapons
- **THEN** it names no substance and no kind

### Requirement: FFD catalog checks
`catalog/ffd.toml` SHALL fail to load when a listed event has no reason, an event is listed twice, or a pattern doesn't compile. (PRD-0005 T-001 T-002)

#### Scenario: Listed twice
- **WHEN** the catalog both includes and excludes event 1
- **THEN** it fails to load

### Requirement: Where an FFD event happened
A fitness-for-duty event outside Power Reactor SHALL count at a facility. A reactor event SHALL be at an operating reactor when a unit it names was in service that day; at a construction site when every unit it names was before commercial operation or is a Part 52 docket `catalog/units.csv` lacks; and at another reactor site otherwise. Its state SHALL be the plant's, as the event gives it. (PRD-0005 T-005)

#### Scenario: Recoded fixture events
- **WHEN** EN 45290 at Browns Ferry 3, EN 45286 moved to Vogtle 3 before its commercial operation, EN 45289 moved to Summer 2 and 3, and fuel cycle EN 45285 are FFD events on 2009-08-24
- **THEN** they count as operating (AL), construction, construction and facility

### Requirement: FFD exposure by state
A build SHALL count each state's in-service unit-days by year, the unit's state from `catalog/units.csv`, with the days of them adult use was legal there and the days a comprehensive medical program was, from `catalog/cannabis.csv`. (PRD-0005 T-005 T-006)

#### Scenario: Browns Ferry 3 and Fermi 2
- **WHEN** both are in service on 2009-08-23 and 2009-08-24
- **THEN** the exposure is AL 2009 with 2 unit-days and no legal days, and MI 2009 with 2 unit-days, both under its 2008-12-04 medical law

### Requirement: The legalization catalog
`catalog/cannabis.csv` (`state,adult_use,sales,medical,source`, `#` lines a header carrying the CC BY-SA 4.0 notice) SHALL list the 50 states and DC once each, and SHALL fail to load on a state that isn't two capital letters, a date that doesn't parse, sales without adult use or before it, or a row without an `https://` source. Adult use and medical SHALL count from their dates on. (PRD-0005 T-004)

#### Scenario: Colorado
- **WHEN** the committed catalog is read
- **THEN** Colorado's adult use counts from 2012-12-10, not 2012-12-09, and its medical program in 2001
- **AND** Idaho has no medical program on 2026-11-18

### Requirement: Scram triggers
Each unretracted event giving a unit a SCRAM Code other than `N` SHALL take a system and a circumstance from `catalog/triggers.toml`'s phrase rules over its title and sentences, updates included: the most upstream root cause the text names, or the signal's system when it names only the trip signal, else `not_named`; and weather, testing, maintenance, human error or `none`. The catalog is the authority for the phrases. (PRD-0007 T-002)

#### Scenario: The root over the signal
- **WHEN** a scram's text says the reactor tripped on a turbine trip caused by a loss of offsite power
- **THEN** its system is `offsite_power`

#### Scenario: A signal alone
- **WHEN** the title is `AUTOMATIC REACTOR TRIP ON TURBINE TRIP` and the text says only that the cause is under investigation
- **THEN** its system is `turbine_generator`

#### Scenario: Nothing named
- **WHEN** the text says only that the cause of the trip is under investigation
- **THEN** its system is `not_named`

### Requirement: Trigger catalog checks
`catalog/triggers.toml` SHALL fail to load when a pattern doesn't compile, a value is unknown, or an override gives no reason, assigns nothing or names an event twice. An override SHALL replace the rules' tag for its event, and a build SHALL fail when one names an event the archive lacks, a retracted one, or one that scrammed no unit. (PRD-0007 T-002)

#### Scenario: Not a scram
- **WHEN** an override names the fixture's EN 45289, which scrammed no unit
- **THEN** the check fails

### Requirement: Trigger accuracy
The committed rules SHALL agree with at least 90% of the system labels and of the circumstance labels on the 150 scram events labeled by hand before any rule existed, and with at least 85% on the 50 validation events drawn after the rules were tuned. (PRD-0007 T-001 T-002)

#### Scenario: Both fixtures
- **WHEN** `uat_scram_triggers_sample` scores the committed rules
- **THEN** both fixtures clear their floors

### Requirement: Scram episodes
A unit scram from at least 10% power SHALL have an episode when the status reports show the unit at least 10 points below that power on the event's day or the next: it runs from that day while the unit stays that far down, and its energy is (pre-scram power − power)% × net capacity × 24 h over those days, whatever cause each carries. A scram the reports don't show has none. (PRD-0007 T-003)

#### Scenario: Back near full power
- **WHEN** Crystal River 3 scrams from 100% on 2009-08-24 and reports 100, 0, 0, 30, 85 and 92 from that day
- **THEN** its episode starts 2009-08-25, runs 4 days, loses 285 points of a day's capacity, and ends `returned`

### Requirement: Episode ends
An episode SHALL end `refueling` at a day attributed to Refueling, `next_scram` when the unit's next scram from at least 10% power is reached, `open` at a gap in the unit's rows or the end of the data, and `returned` at a day back within 10 points. (PRD-0007 T-003)

#### Scenario: Into refueling
- **WHEN** Browns Ferry 3 is down from the event's day and its reports name a refueling outage from the fourth day
- **THEN** its episode runs 3 days and ends `refueling`

### Requirement: The scram series
A build SHALL store the scram series beside the trends: every unit scram on a catalog unit with its tags and episode, and each catalog unit's in-service days per month from the build's first month. (PRD-0007 T-005)

#### Scenario: A unit off one page
- **WHEN** Peach Bottom 3 is off one of nine August 2009 pages
- **THEN** its days for that month read 8

### Requirement: Observed and attributed layers
In the attributed layer a unit-day SHALL carry its attributed family, and in the observed layer it SHALL keep that family only when its method is `precedence_v1` and count as Unattributed otherwise. The barcode cells and monthly series `api` serves are built per layer this way. (PRD-0002 T-013 T-014)

#### Scenario: An inherited day
- **WHEN** Diablo Canyon 2 reads `Refueling Outage` on 2025-07-01 and `Increasing Power` at 30% on 07-03
- **THEN** 07-03 is Refueling in the attributed layer and Unattributed in the observed layer, both at 30%

#### Scenario: A resolved reference
- **WHEN** Crystal River 3 reads only `See EN #45286`, a scram
- **THEN** the day is Forced in the attributed layer and Unattributed in the observed layer

### Requirement: Derived version
Every build SHALL record a `derived_version`, 16 lowercase hex digits hashing the ledger version constant and the catalog files the binary embeds (`tags.toml`, `unclassified.tsv`, `units.csv`, `capacity.csv`, `facilities.csv`, `xref.toml`, `ffd.toml`, `cannabis.csv` and `triggers.toml` under `catalog/`), so a new ledger version or a catalog change changes it. (PRD-0002 T-008 T-009, PRD-0003 T-004 T-005, PRD-0005 T-006, PRD-0007 T-005)

#### Scenario: A stable stamp
- **WHEN** the derived version is computed twice by one binary
- **THEN** both are the same 16 hex digits

### Requirement: Deterministic builds
Two builds from the same raw store SHALL produce identical rows and identical summaries. (PRD-0002 T-008)

#### Scenario: Rebuilding twice
- **WHEN** the ledger is rebuilt twice over the fixture raw store
- **THEN** both builds hold the same rows and summary in different tables
- **AND** only the second build's table remains

### Requirement: Full rebuilds swap in when complete
A full rebuild SHALL write a fresh build table named `unit_day_<microseconds since epoch>` and its event tables `event_<same number>` and `event_unit_<same number>`, make them current together with the `unit` table in one transaction only once complete, and then drop every other build's tables. A rebuild that fails SHALL leave the previous build current and serving, and the next rebuild SHALL drop its partial tables. (PRD-0002 T-008, PRD-0003 T-003 T-004)

#### Scenario: A page that fails to parse
- **WHEN** a rebuild meets a stored page for 2025-07-17 that isn't a status page
- **THEN** it fails naming 2025-07-17
- **AND** the previous build stays current with its rows, beside the partial table

#### Scenario: Two rebuilds
- **WHEN** the ledger is rebuilt twice
- **THEN** only the second build's `event_<n>` and `event_unit_<n>` remain, numbered as its `unit_day_<n>`

### Requirement: Serving through a record-shape change
A current-build record the running binary cannot parse SHALL count as no build for refresh, so the next refresh rebuilds, while the build table it names SHALL keep serving every data route and SHALL NOT be dropped before a new build is current. (PRD-0002 T-011 T-020)

#### Scenario: A deploy changes the record's shape
- **WHEN** the current record loses its summary and the next rebuild fails
- **THEN** the binary reads no current build
- **AND** the table the record names still serves and still holds its rows

### Requirement: Refresh rebuilds, updates or does nothing
Refreshing the ledger SHALL do nothing when the current build's `derived_version` and raw digest match the binary's and the raw store's; SHALL update the current build in place when only the digest moved and the build stored its unit-month and event hashes; and SHALL rebuild otherwise. A refresh SHALL quarantine unknowns either way, and SHALL log once that it rebuilds over a record in another shape. (PRD-0002 T-008 T-011 T-019 T-020, PRD-0003 T-003)

#### Scenario: A refetch, then new content
- **WHEN** a page is recorded again with the same bytes, and later a new page is stored
- **THEN** the first refresh writes nothing
- **AND** the second writes a build with more rows and a new raw digest

#### Scenario: Another version
- **WHEN** the current build's version differs from the binary's
- **THEN** a refresh rebuilds into a new table, which becomes current
- **AND** when the record is in an older shape, that refresh logs it once, and reading `/status` meanwhile logs nothing

### Requirement: In-place updates match a full rebuild
An in-place update SHALL leave the current build serving exactly what a full rebuild of the same raw store serves (rows, both layers' barcodes and monthly series, the trends, the FFD and scram series, events with their identities and unit rows, and removed unit-months and events), rewriting only the unit-months and events that changed, and SHALL record the new digest and summaries under the same table. (PRD-0002 T-019, PRD-0003 T-003 T-004, PRD-0005 T-006, PRD-0006 T-003, PRD-0007 T-005)

#### Scenario: A new page, then pages withdrawn
- **WHEN** a page for 2025-07-16 arrives, and later both July pages are recorded as 404s
- **THEN** each refresh updates the same table
- **AND** after each, the table's rows, barcode tags and monthly series equal a fresh rebuild's

#### Scenario: A new event page, an edit, and a page withdrawn
- **WHEN** the 2026-02-11 event page arrives, and later the 2020-10-20 page is recaptured with EN 54960 edited and 2026-02-11 is recorded as a 404
- **THEN** each refresh updates the same event table, which then equals a fresh build's event set
- **AND** EN 54960 reads as edited, and the three 2026-02-11 events are gone

#### Scenario: A reactor event page arrives, then is withdrawn
- **WHEN** the 2009-08-24 event page arrives, and later is recorded as a 404
- **THEN** the first refresh adds its reactor events' unit rows, equal to a fresh build's
- **AND** the second removes them

### Requirement: An interrupted update repeats
An update that fails part way SHALL leave the current build's recorded raw digest unchanged, so the next refresh compares against the build's old unit-month hashes and writes those months again. (PRD-0002 T-019)

#### Scenario: A write fails mid-update
- **WHEN** an in-place update fails after rewriting some unit-months
- **THEN** the current build still records the old digest
- **AND** the next refresh updates the same months again
