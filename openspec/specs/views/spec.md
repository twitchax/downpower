# views Specification

## Purpose
The pages a reader sees: the home page's filters, fleet barcode and stacked area of reported power reduction, the fleet trends and the fitness-for-duty page, which share one state kept in the URL so every view is a permalink, the events explorer, and `/status`, which says what the ledger rests on.

## Requirements
### Requirement: Pages
The site SHALL render the home page at `/`, `/trends`, `/scrams`, `/events`, one event at `/events/{number}`, `/ffd` and `/status`, each under a nav linking `downpower`, `trends`, `scrams`, `events`, `ffd` and `status` and declaring the site's icon, `/favicon.svg`; any other path SHALL render `Page not found.` (PRD-0002 T-013 T-015, PRD-0004 T-004 T-005, PRD-0005 T-008, PRD-0006 T-005, PRD-0007 T-006)

#### Scenario: Both pages render
- **WHEN** `downpower serve` is up over the fixture raw store
- **THEN** `/` carries the barcode's canvas and the stacked area
- **AND** `/status` carries the heading `Status`
- **AND** `/` links `/favicon.svg`, which the site serves

### Requirement: URL state
The home page's views SHALL share one state kept in the URL query: `from` and `to` (`YYYY-MM-DD`, running to the data's edge when absent), `layer` (`attributed`, the default, or `observed`), `families`, the slugs shown in color, comma-separated in stack order (all when absent), `events=1` for the barcode's markers and `table=1` for the charts' tables. A parameter at its default SHALL be left out, so the default state has an empty query. (PRD-0002 T-013, PRD-0004 T-006, PRD-0006 T-004, 09d28a3)

#### Scenario: A permalink
- **WHEN** the state is 2010-01-01 to 2020-12-31, the observed layer, only Forced and Grid, markers on and the table shown
- **THEN** its query is `?from=2010-01-01&to=2020-12-31&layer=observed&families=grid,forced&events=1&table=1`
- **AND** that query parses back to the same state

### Requirement: One state across pages
`/trends`, `/scrams` and `/ffd` SHALL read the home page's URL state, and a control on any of the four SHALL replace the state while keeping the page's path, so a link carries the range, layer and families from one to another. (PRD-0006 T-004, PRD-0007 T-006)

#### Scenario: A range on the trends page
- **WHEN** the reader presses 10 yr on `/trends`
- **THEN** the URL stays on `/trends` with the new `from`

### Requirement: Mangled links still open
A parameter that doesn't parse SHALL keep its default, and an unknown family slug SHALL be dropped. The family slugs are `refueling`, `fuel-cycle`, `maintenance`, `equipment`, `environmental`, `grid`, `forced` and `unattributed`. (PRD-0002 T-013)

#### Scenario: An old link
- **WHEN** a link carries `from=last tuesday`, `layer=both` and `families=forced,heat`
- **THEN** it opens with no start date, the attributed layer, and Forced alone in color

### Requirement: Filters
One row above both charts SHALL hold a layer toggle (Attributed, Observed), range presets (All, 10 yr, 5 yr, 1 yr, each counting whole years back from the viewer's today with no end date), and `from` and `to` date inputs, and below it a legend of the eight families in stack order, each entry toggling its family, with `Show all` once any is hidden. Every control SHALL replace the URL's state in history. (PRD-0002 T-013)

#### Scenario: A preset from a leap day
- **WHEN** the viewer presses 1 yr on 2024-02-29
- **THEN** the range starts on 2023-02-28
- **AND** 10 yr on 2026-11-14 starts on 2016-11-14

### Requirement: Family order and colors
The charts and legend SHALL stack and list families bottom up as Refueling, Fuel cycle, Maintenance, Equipment, Environmental, Grid, Forced, Unattributed, each in its own color from `style/_families.scss`, and a hidden family SHALL recede to gray in the barcode and drop out of the stacked area. (PRD-0002 T-013 T-014)

#### Scenario: Hiding Refueling
- **WHEN** Refueling is hidden
- **THEN** Forced sits on the stacked area's baseline
- **AND** a barcode span only Refueling reduced paints in the hidden gray

### Requirement: The fleet barcode
The barcode SHALL draw `api`'s buffer for the chosen layer on a canvas, one row per unit and one device-pixel column per span of days in the chosen range. The visible family that lost the most in a span SHALL color it, at an opacity rising with the mean depth of the cut; a span at full power SHALL show a faint track and a span with no data nothing. `crates/downpower-app/src/paint.rs` is the authority for the opacities. (PRD-0002 T-013)

#### Scenario: Two units over four days
- **WHEN** one unit is refueling at 0% on days 1 and 2 and at full power after, and another is out of service until a forced cut to 50% on day 4, drawn two columns wide
- **THEN** the first row shows Refueling at near-full opacity, then the track
- **AND** the second row shows nothing, then Forced, with a clear gap between rows

### Requirement: Ranges clamp to the data
Both charts SHALL clamp the range to the days or months the data covers, and an inverted range SHALL show everything. (PRD-0002 T-013 T-014)

#### Scenario: A range past the data
- **WHEN** the barcode covers 2024-01-01 to 01-04 and the range is 2024-01-02 to 2030-01-01
- **THEN** it shows 2024-01-02 to 01-04
- **AND** a range from 01-04 back to 01-02 shows all four days

### Requirement: Barcode readout
Hovering the barcode, or moving a cursor over it with the arrow keys (starting at the newest day of the first row, Escape clearing it), SHALL show the cell's power, as precise as its bucket allows, and its family or `full power`, with the unit and date; once the pointer rests on a cell, the readout SHALL add the day's reason and its provenance from `api`'s unit-day route. (PRD-0002 T-013 T-021, PRD-0003 T-005)

#### Scenario: Keyboard cursor
- **WHEN** a reader focuses the barcode and presses an arrow key
- **THEN** the cursor starts at the newest day of the first row and stays inside the plot

#### Scenario: Provenance in words
- **WHEN** the row behind a cell is a `reactor_trip` by `precedence_v1`
- **THEN** the readout says `reactor trip · named on the day`, and a bucket of 50% reads `50–53%`

#### Scenario: A resolved reference in words
- **WHEN** the row is a `scram` by `xref_en_v1`
- **THEN** the readout says `scram · from the event report it cites`

#### Scenario: A transition with nothing to inherit
- **WHEN** the row is a transition by `inherit_14d` with no cause in the 14 days before
- **THEN** the readout says `a transition, with no cause in the 14 days before`, not that it inherited

### Requirement: The readout's event
When a day's cause came from an event, the barcode readout SHALL name it and the field that decided it (`from EN 57537: scram, SCRAM Code A/R`), and a click on the day or Enter SHALL open `/events/{number}` for that event, or for the first event the day's reason cites when none decided it. (PRD-0004 T-007)

#### Scenario: A scram from its event
- **WHEN** the readout rests on Browns Ferry 3 on 2025-02-09, `See EN 57537`, a scram from SCRAM Code `A/R`
- **THEN** it reads `from EN 57537: scram, SCRAM Code A/R`, and a click opens `/events/57537`

### Requirement: Event markers
With `events=1` in the URL, which the legend's `Events` toggle sets and clears and which is off by default, the barcode SHALL mark each unit-day a mapped, unretracted reactor event names, a one-pixel tick for an event and a diamond for a scram, and the readout SHALL list the events on the hovered cell, a scram first, a click or Enter opening the first. (PRD-0004 T-006)

#### Scenario: A scram marked
- **WHEN** markers are on and the pointer rests on Browns Ferry 3 on 2009-08-24
- **THEN** a diamond marks the cell and the readout lists `EN 45290 (scram)`

### Requirement: A pixel spanning several days
When one pixel spans several days, the readout SHALL point at the span's deepest cut, or its middle day when nothing is cut, and SHALL say `1 of N days in this pixel`. (PRD-0002 T-013)

#### Scenario: A refueling span
- **WHEN** a pixel spans a refueling day at 0% and two full-power days
- **THEN** the readout points at the refueling day

### Requirement: The stacked area
Beneath the barcode the home page SHALL show the reported power reduction in TWh per month by family, from `api`'s monthly series for the chosen layer and range, as steps stacked in family order with a round-numbered scale over the visible peak and year marks on each January. (PRD-0002 T-014)

#### Scenario: Round ticks
- **WHEN** the visible peak is 7.3 TWh
- **THEN** the scale runs 0, 2, 4, 6, 8

### Requirement: Crosshair readout and table
Hovering the stacked area SHALL show a crosshair and a readout of that month: each visible family's TWh and their total. A `Table` toggle, kept in the URL as `table=1`, SHALL list every family's TWh and share over the range, hidden families included, with an `All` row. (PRD-0002 T-014, 09d28a3)

#### Scenario: Totals over the range
- **WHEN** the series holds 2 TWh of Refueling in November 2023 and 1 TWh in February 2024
- **THEN** the table's Refueling total over all months is 3 TWh
- **AND** a range from 2024-01-15 shows January and February 2024 only

### Requirement: Readouts stay beside the pointer
A hover readout SHALL sit just right of the pointer, and near the plot's right edge SHALL flip to end just left of it, inside the plot either way. (PRD-0002 T-013 T-014)

#### Scenario: Near the right edge
- **WHEN** the pointer is 1094px into a 1216px-wide barcode
- **THEN** the readout's right edge sits at 1082px, 12px left of the pointer, whatever the readout's width

### Requirement: The reduction's label
The site SHALL label lost output "Reported power reduction" and never call it heat loss. (PRD-0002 T-014)

#### Scenario: The home page
- **WHEN** `/` is rendered
- **THEN** it reads `Reported power reduction`

### Requirement: Chart loading states
Each chart SHALL say it is loading until its data arrives and show the error when its route fails, such as the 503 `api` answers while the ledger builds. (PRD-0002 T-013 T-014)

#### Scenario: Before the first build
- **WHEN** the home page loads while `/api/barcode/attributed` answers 503
- **THEN** the barcode shows that the route answered 503 instead of drawing

### Requirement: The trends page
`/trends` SHALL show, over the URL state's range and below the home page's filters without the markers toggle, four bar charts: reported power reduction by year as a share of net capacity stacked by visible family, the same share by month of the year, refueling outage length by year (a box from the lower to the upper quartile, a median line, the count), and EN scrams per 100 operating unit-years by year, automatic and manual. (PRD-0006 T-004)

#### Scenario: The page served
- **WHEN** `downpower serve` is up over the fixture raw store
- **THEN** `/trends` carries `Fleet trends`, `Reported power reduction by year`, `By month of the year`, `Refueling outage length` and `Scrams`

### Requirement: Trend readouts, tables and the partial year
Hovering a trends chart's column SHALL show its values, the top of a stack first, and `table=1` SHALL show every chart's table. A year the data or the range doesn't cover whole SHALL be labeled with a star, drawn faded and read as partial. (PRD-0006 T-004)

#### Scenario: Forced hidden, 2025 partial
- **WHEN** 2024 and a partial 2025 each lost 6% of capacity to Refueling and Forced is hidden
- **THEN** the columns read `2024` and `2025*`, the second drawn faded
- **AND** its readout reads `2025 (partial) · 6.00% of capacity`

#### Scenario: Scram rates
- **WHEN** a year holds 3,653 unit-days, 20 automatic scrams and 5 manual
- **THEN** its readout reads `2024 · 250.0 per 100 unit-years over 10.0`

### Requirement: The FFD page
`/ffd` SHALL show over the URL state's range: FFD events by year by substance (drug-related, alcohol, neither named) with construction-site, other reactor and facility events counted apart; drug-related and all FFD events per 100 operating unit-years; the share of operating unit-years in states with legal adult use and a medical program; a state table; breakdowns by role, kind and test; and the caveats. (PRD-0005 T-007)

#### Scenario: The page served
- **WHEN** `downpower serve` is up over the fixture raw store
- **THEN** `/ffd` carries `Fitness for duty`, `FFD events by year`, `Legal cannabis`, `26.719(b)` and a link to `/events?ffd=1`

### Requirement: FFD rates and the 2008 rewrite
Each `/ffd` chart SHALL mark Part 26's rewrite, effective 2008-04-30, with one dashed rule in 2008's column, and SHALL read rates per 100 operating unit-years (in-service unit-days over 365.25) and shares as percentages of them, with `table=1` showing every chart's table. (PRD-0005 T-007)

#### Scenario: 100 unit-years
- **WHEN** a year holds 36,525 operating unit-days, 10 drug-related events and 30 in all, a quarter of them in adult-use states
- **THEN** its rate row reads `100.0`, `10.0` and `30.0`, and its share row `25.00%`

### Requirement: The FFD state table
The state table SHALL list each state's operating unit-years, FFD and drug-related events at operating reactors, its adult-use, sales and medical dates, and its drug-related events and their rate per 100 unit-years before adult use and since, `—` where there is none, each state linking `/events?ffd=1&state=` its code. (PRD-0005 T-007)

#### Scenario: Split at adult use
- **WHEN** Illinois has 3,653 unit-days and one drug-related event before adult use and 3,652 and one after
- **THEN** its row reads 10.0 per 100 both before and after

### Requirement: Phone widths
Under 640px every page SHALL fit the screen: the filter rows wrap, axis year labels thin to the five-year marks, and a label in an axis's last twentieth hangs left of its tick. (PRD-0005 T-007)

#### Scenario: The last label
- **WHEN** axis labels sit at 98% and 95% of the width
- **THEN** the first hangs left and the second doesn't
- **AND** on a phone 2026's label hides while 2020's stays

### Requirement: The scrams page
`/scrams` SHALL show, over the URL state's range and below its range control, three bar charts by year: scrams per 100 operating unit-years stacked by system, the energy scram episodes lost as a share of net capacity stacked by system, and scrams tied to testing, maintenance or an error per 100 unit-years; then a unit table and caveats. `/trends`'s scram chart SHALL link it under the same query. (PRD-0007 T-006)

#### Scenario: The page served
- **WHEN** `downpower serve` is up over the fixture raw store
- **THEN** `/scrams` carries `Scram triggers`, `Scrams by trigger`, `Output lost to scrams`, `Scrams tied to testing, maintenance or an error` and `By unit`, and `/trends` carries `by trigger →`

### Requirement: Scram readouts and tables
Each scram chart's readout SHALL head its year (`(partial)` when the days shown leave part out) with the total and list each system or circumstance with its count, and its table SHALL give every year's counts and rates. The tied chart's readout and table SHALL add weather. Every system SHALL keep its color in stack order, from feedwater's green to not named's gray. (PRD-0007 T-006)

#### Scenario: Rates
- **WHEN** a year of 10.0 unit-years holds 4 feedwater and 6 turbine and generator scrams, 2 during testing, 1 maintenance, 1 human error and 3 weather
- **THEN** the rate readout heads `2024 · 100.0 per 100 unit-years over 10.0` and the tied one `2024 · 40.0 per 100 unit-years tied to testing, maintenance or an error`

### Requirement: The scram unit table
`/scrams` SHALL list the units with 365 or more in-service days over the range, most scrams per unit-year first, each with its unit-years, scrams, rate per 100, median episode, lost GWh and leading named system, and linking `/events` filtered to its scrams over the range. (PRD-0007 T-006)

#### Scenario: Too little exposure
- **WHEN** three units hold 3,653, 1,000 and 200 unit-days and 5, 4 and 3 scrams
- **THEN** the table lists the second, then the first

### Requirement: Explorer filters in the URL
`/events` SHALL keep its filters in the query: `from` and `to`, `category` (comma-separated group slugs, the NRC's renamed pairs merged), `state`, `facility`, `unit`, `cfr`, `class`, `scram=1`, `ffd=1`, `trigger` (comma-separated system slugs), `q` and `page`, each left out at its default, and a parameter that doesn't parse SHALL keep its default. A change of filter SHALL go back to the first page. (PRD-0004 T-002 T-004, PRD-0005 T-007, PRD-0007 T-007)

#### Scenario: A round trip
- **WHEN** every filter is set, words and 10 CFR code included
- **THEN** the query parses back to the same filters, percent-encoded where it must be

#### Scenario: A mangled link
- **WHEN** a link carries `from=yesterday&state=Texas&unit=498&page=0`
- **THEN** each keeps its default and the page still opens

#### Scenario: A trigger list
- **WHEN** a link carries `trigger=offsite_power,fire,feedwater`
- **THEN** the filter holds feedwater and offsite power, and `/events?trigger=feedwater` renders the trigger select

### Requirement: The explorer page
`/events` SHALL show one row of filter controls, the category groups as toggles that double as the legend, a stacked area of the selected events per month in eight series (the seven largest groups and Other, in the cause families' hues with Other in gray) with a month readout on hover, and the page's events newest first, each with its date, group, place or units, title linking `/events/{number}`, 10 CFR codes and `scram` and `retracted` chips, and links to the pages either side. (PRD-0004 T-004)

#### Scenario: Scrams in Alabama
- **WHEN** `/events?state=AL&scram=1` renders
- **THEN** the scram toggle is pressed and the list holds EN 45290

### Requirement: The event page
`/events/{number}` SHALL show the event's dates, category, place, emergency class, last update, callers and a scram's trigger; its 10 CFR sections linking the explorer filtered to each; a reactor event's unit table (SCRAM Code, criticality, initial and current power and mode) and each unit's attributed barcode strip 60 days either side; the updates, the text, a retraction mark and the NRC page's link; `No event {number}` when the API has none. (PRD-0004 T-005, PRD-0007 T-007)

#### Scenario: A scram
- **WHEN** `/events/45290` loads
- **THEN** its unit table reads Browns Ferry 3 `M/R` and it links the 2009-08-24 NRC page at `#en45290`

#### Scenario: A scram's trigger
- **WHEN** an event's trigger is feedwater, during testing
- **THEN** its facts read `Feedwater and condensate, during testing`

### Requirement: Status page ledger panel
`/status` SHALL show the current build's unit-days, span, build time and derived version; its reported power reduction in TWh and its reduced unit-days without a capacity; dates by source (with reasons, inside the hold, EIA only, missing) with counts and shares; the NRC/EIA disagreement rate; and the quarantined tags and unit names with their uses, or `Nothing quarantined` when there are none. (PRD-0002 T-015)

#### Scenario: Plain numbers
- **WHEN** the panel renders 1016840 unit-days built at `2026-11-14T20:22:24.705325Z` with 12 disagreements in 714972
- **THEN** it reads `1,016,840`, `2026-11-14 20:22 UTC` and `0.002%`

#### Scenario: No build
- **WHEN** `/api/status` has no build and no serving table
- **THEN** the panel says there is no build yet

#### Scenario: Rebuilding after a deploy
- **WHEN** `/api/status` has no readable build but names a serving table
- **THEN** the panel says the site serves the previous build until the new one is done

### Requirement: Status page scram check
Once a build records the scram check, the ledger panel SHALL state both agreement rates as `ledger` defines the outcomes: EN scrams `dropped` or `named` of those plus `missed`, and report scrams `event` of those plus `no_scram_code` and `missed`. (PRD-0003 T-007)

#### Scenario: The scram check
- **WHEN** a build counts 1,200 dropped, 51 named and 20 missed EN scrams, and 1,208 event, 50 no-scram-code and 529 missed report scrams
- **THEN** the panel reads `1,251 of 1,271` (98.4%) and `1,208 of 1,787` (67.6%)
- **AND** a build from before the check shows no scram sentence

### Requirement: Status page scram dataset
Once a build records the scram dataset's counts, the ledger panel SHALL state its span, the scrams both sides list of those plus the dataset-only and ours-only ones, and the known differences left aside. (PRD-0003 T-008 T-010)

#### Scenario: The dataset
- **WHEN** a build counts 708 agreeing, 9 dataset-only, 8 ours-only and 24 known scrams from 2011-01-01 to 2025-05-12
- **THEN** the panel reads `708 of 725 scrams (97.7%), leaving aside 24 known differences`

### Requirement: Status page event archive
When the build holds an event archive, `/status` SHALL show its pages, those reading "No events found", its events, every page that failed to parse (the first 10 by date and a count of the rest) or `Every page parsed`, its events by category, most first, and the quarantined facility names with their events, or `Nothing quarantined`. (PRD-0003 T-010)

#### Scenario: The archive
- **WHEN** a build read 7,325 pages, 639 empty, holding 19,725 events, and the page for 2020-10-22 failed
- **THEN** the panel says so and names 2020-10-22

### Requirement: Status page runs
`/status` SHALL list the runs `api` returns, each marked passed or failed, with its start time, its status-page and event-report counts (`stored`, `unchanged`, `missing`, `settled`) or `not fetched`, the last `NUC_STATUS` day and the scram dataset's last day when it fetched them, whether it rebuilt or left the ledger unchanged, and its errors. (PRD-0002 T-015 T-019, PRD-0003 T-008)

#### Scenario: A run recorded before unchanged counts existed
- **WHEN** a run's status counts lack `unchanged`
- **THEN** it reads `status pages: 1 stored, 0 unchanged, 0 missing, 61 settled`
