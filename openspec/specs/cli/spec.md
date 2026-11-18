# cli Specification

## Purpose
The `downpower` binary's subcommands, flags and environment, and the `cargo xtask` commands maintainers run against the raw store and the committed `catalog/` tables.

## Requirements
### Requirement: Subcommands
`downpower` SHALL offer `serve`, which also runs when no subcommand is given, `fetch`, `backfill` and `rebuild`, with `--help` and `--version`. (PRD-0002 T-002 T-010)

#### Scenario: No subcommand
- **WHEN** `downpower` runs with no arguments
- **THEN** it serves, as `downpower serve` does

#### Scenario: Backfill and rebuild parse
- **WHEN** `downpower backfill` and `downpower rebuild` are parsed
- **THEN** each is its own subcommand, `backfill` at a 5-second interval by default

### Requirement: Data dir and logging
Every subcommand SHALL use `DOWNPOWER_DATA_DIR` as its data dir, `./data` when it is unset or empty, creating it on first use, with the raw store under `raw/` and the database under `downpower.db`. Logs SHALL go to stderr, filtered by `RUST_LOG` and at `info` by default. (PRD-0001 T-001, PRD-0002 T-001)

#### Scenario: Unset, empty or set
- **WHEN** `DOWNPOWER_DATA_DIR` is unset, empty, or `/data`
- **THEN** the data dir is `data`, `data`, or `/data`

#### Scenario: A fresh nested dir
- **WHEN** the data dir doesn't exist yet
- **THEN** opening the store creates it and `downpower.db` inside it

### Requirement: serve
`downpower serve` SHALL open the database before binding, so a missing or unwritable data dir fails before the health check can pass, then start the update loop as `update-loop` describes and listen on `LEPTOS_SITE_ADDR`, serving the pages `views` describes and the routes `api` describes. (PRD-0001 T-001, PRD-0002 T-011 T-015)

#### Scenario: The binary over a fixture raw store
- **WHEN** `downpower serve` starts in a temporary data dir holding the fixture raw store
- **THEN** the barcode answers 200 once the start-up rebuild lands
- **AND** `/`, `/status` and `/api/status` answer, with one recorded run

### Requirement: fetch
`downpower fetch --series status` or `--series events` SHALL fetch every unsettled page of that series as `raw-store` describes, from `--from` (default 1999-01-01) through `--to` (default today, UTC), starting requests `--interval-secs` apart (default 5). `--series nuc-status` SHALL fetch `NUC_STATUS` in one request, ignoring the dates and interval. (PRD-0002 T-002 T-007)

#### Scenario: Defaults
- **WHEN** `downpower fetch --series events` is parsed
- **THEN** it covers 1999-01-01 through today at a 5-second interval

#### Scenario: The bulk file
- **WHEN** `downpower fetch --series nuc-status` is parsed
- **THEN** it names no NRC report to page through

### Requirement: The interval floor
`--interval-secs` below 5 SHALL be rejected, for `fetch` and `backfill` alike. (PRD-0002 T-002 T-010)

#### Scenario: Asking for 4 seconds
- **WHEN** `fetch --series status --interval-secs 4` or `backfill --interval-secs 4` is given
- **THEN** both are rejected, and `--interval-secs 60` is accepted

### Requirement: backfill
`downpower backfill` SHALL fetch every status page and then every Event Notification page from 1999-01-01 through today, then a fresh `NUC_STATUS`, then rebuild as `downpower rebuild` does, failing on the first step that fails. Each fetch skips what earlier runs settled, so a rerun costs only the unsettled pages. (PRD-0002 T-010)

#### Scenario: A rerun after an interruption
- **WHEN** `downpower backfill` runs again after stopping part way
- **THEN** it requests only the pages no earlier run settled, then refetches `NUC_STATUS` and rebuilds

### Requirement: rebuild
`downpower rebuild` SHALL build the ledger from the raw store into a fresh build under the fail policy `ledger` describes, exiting non-zero and naming the first unknown unit name or tag, and SHALL fail while another process, such as `serve`, holds the database. (PRD-0002 T-010)

#### Scenario: A clean rebuild
- **WHEN** `downpower rebuild` runs over the fixture raw store
- **THEN** it exits 0, logging `rebuild done`
- **AND** the current build carries the binary's derived version and the raw store's digest

#### Scenario: An unknown unit or tag
- **WHEN** the stored page names `Mystery 9`, or carries `Zorbulating the Flux Capacitor`
- **THEN** `downpower rebuild` exits non-zero naming `unknown unit "Mystery 9"`, or the tag

### Requirement: xtask data dir
`cargo xtask` SHALL read the raw store under `--data-dir`, else `DOWNPOWER_DATA_DIR`, else `data`, and `reasons`, `summary`, `eia` and `events` SHALL fail with a pointer to `downpower fetch` when no raw store is there. (PRD-0002 T-005 T-006 T-007, PRD-0003 T-001)

#### Scenario: No raw store
- **WHEN** `cargo xtask summary` runs where there is no `raw/manifest.jsonl`
- **THEN** it fails, saying to run `downpower fetch` first

### Requirement: xtask catalog commands
`cargo xtask reasons` SHALL rewrite `catalog/reasons.tsv` (`tag`, `uses`, `below_100`, most used first) from the newest capture of every stored status page; `unclassified` SHALL rewrite `catalog/unclassified.tsv` with every snapshot tag no key or rule matches, sorted; and `rules` SHALL print what the keys, each rule and the list catch, with shares of uses and `--examples` tags each (8 by default). (PRD-0002 T-005)

#### Scenario: A new tag appears
- **WHEN** a fetch brings a tag no key or rule matches and `reasons`, then `unclassified`, run
- **THEN** the tag is in `catalog/reasons.tsv` and listed in `catalog/unclassified.tsv`, for review before commit

### Requirement: xtask summary and eia
`cargo xtask summary` SHALL build the event set and the ledger as a rebuild does, under the fail policy, and print lost output by family, method and fine cause, and for days citing an EN, by the cause their references resolved to or why none did. `cargo xtask eia` SHALL compare the stored status pages with the newest stored `NUC_STATUS` and print their disagreement rate and the in-service unit-days only EIA reports. (PRD-0002 T-006 T-007 T-009, PRD-0003 T-005)

#### Scenario: A summary over the archive
- **WHEN** `cargo xtask summary` runs over a raw store whose units and tags are all known
- **THEN** it prints the in-service unit-days and each family's unit-days below 100%, unit-years, TWh and share

### Requirement: xtask scrams
`cargo xtask scrams` SHALL build the event set and the ledger under the fail policy, run the scram check as `ledger` describes, and print both directions' counts and agreement overall and by year, every `missed` EN scram, and `--examples` report scrams (20 by default) of each of `missed` and `no_scram_code`. (PRD-0003 T-007)

#### Scenario: Over the archive
- **WHEN** `cargo xtask scrams` runs over a raw store whose units, tags and facilities are all known
- **THEN** it prints the share of EN scrams the status reports show and the share of report scrams an event reports

### Requirement: xtask events
`cargo xtask events` SHALL parse every stored Event Notification page and print the archive's counts (pages with events, empty pages by year, events repeated across pages, events by category and with no 10 CFR section, unused event numbers), the update markers' kinds and parse rates, and the events identified by kind, and SHALL fail when any page fails to parse or names a facility the catalogs lack, printing the first 20 of each. (PRD-0003 T-001 T-002 T-004)

#### Scenario: A page departs from the template
- **WHEN** `cargo xtask events` runs over a raw store holding an event page with a label the parser doesn't know
- **THEN** it prints `FAILED <date>:` with the error naming the label
- **AND** exits non-zero, counting the failed pages

#### Scenario: A facility the catalogs lack
- **WHEN** a stored Power Reactor event names a site no catalog lists
- **THEN** it prints `UNKNOWN Power Reactor "<name>" unit <n>` with its events
- **AND** exits non-zero, saying to add it to `catalog/units.csv` or `catalog/facilities.csv`

### Requirement: xtask events rewrites the facility fixture
`cargo xtask events` SHALL rewrite `crates/downpower-ingest/tests/fixtures/events/facilities.tsv` with one sorted line per facility the stored events name: category, state, name, and a power reactor's unit or a non-power reactor's license, the field blank otherwise. The identity test holds the catalogs against it. (PRD-0003 T-004)

#### Scenario: A new spelling on a fetched page
- **WHEN** a newly stored page lists Hematite as `Combustion Engineering Inc.` in MO
- **THEN** the fixture gains a `Fuel Cycle Facility` line for `COMBUSTION ENGINEERING INC.` in MO, and the identity test fails until `catalog/facilities.csv` names it

### Requirement: xtask capacity
`cargo xtask capacity <dir>` SHALL rewrite `catalog/capacity.csv` from the EIA-860 zips in `<dir>` (`eia860a1999.zip`, `eia860a2000.zip`, `eia8602001.zip` to `eia8602006.zip`), rating each unit-year from its own year's file or else the nearest later one, and SHALL fail when a year's file lists a unit twice. (PRD-0002 T-009)

#### Scenario: A unit missing from one year's file
- **WHEN** a unit is absent from the 2003 file but present in 2004's
- **THEN** its 2003 row takes the 2004 rating, with `eia860_year` 2004

### Requirement: xtask crosscheck
`cargo xtask crosscheck <dir>` SHALL compare the stored 1999 to 2016 status pages with the CSVs in `<dir>`, the `Data` directory of a clone of `mike-welch/NRC_Power_Reactor_Status_Reports`, matching units by docket with the old reading-room names included, and print power and reason agreement by year. It SHALL fail when `<dir>` holds no CSV rows. (PRD-0002 T-012)

#### Scenario: An empty directory
- **WHEN** `<dir>` holds no CSV rows
- **THEN** the command fails with "no CSV rows"
