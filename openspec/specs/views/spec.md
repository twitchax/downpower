# views Specification

## Purpose
The pages a reader sees: the home page's filters, fleet barcode and stacked area of reported power reduction, which share one state kept in the URL so every view is a permalink, and `/status`, which says what the ledger rests on.

## Requirements
### Requirement: Pages
The site SHALL render the home page at `/` and the status page at `/status`, each under a nav linking `downpower` to `/` and `status` to `/status`, and any other path SHALL render `Page not found.` Each page SHALL declare the site's icon, `/favicon.svg`. (PRD-0002 T-013 T-015)

#### Scenario: Both pages render
- **WHEN** `downpower serve` is up over the fixture raw store
- **THEN** `/` carries the barcode's canvas and the stacked area
- **AND** `/status` carries the heading `Status`
- **AND** `/` links `/favicon.svg`, which the site serves

### Requirement: URL state
The home page's views SHALL share one state kept in the URL query: `from` and `to` (`YYYY-MM-DD`, running to the data's edge when absent), `layer` (`attributed`, the default, or `observed`), and `families`, the slugs shown in color, comma-separated in stack order (all when absent). A parameter at its default SHALL be left out, so the default state has an empty query. (PRD-0002 T-013)

#### Scenario: A permalink
- **WHEN** the state is 2010-01-01 to 2020-12-31, the observed layer, and only Forced and Grid
- **THEN** its query is `?from=2010-01-01&to=2020-12-31&layer=observed&families=grid,forced`
- **AND** that query parses back to the same state

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
Hovering the stacked area SHALL show a crosshair and a readout of that month: each visible family's TWh and their total. A `Table` toggle SHALL list every family's TWh and share over the range, hidden families included, with an `All` row. (PRD-0002 T-014)

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

### Requirement: Status page runs
`/status` SHALL list the runs `api` returns, each marked passed or failed, with its start time, its status-page and event-report counts (`stored`, `unchanged`, `missing`, `settled`) or `not fetched`, the last `NUC_STATUS` day when it fetched one, whether it rebuilt or left the ledger unchanged, and its errors. (PRD-0002 T-015 T-019)

#### Scenario: A run recorded before unchanged counts existed
- **WHEN** a run's status counts lack `unchanged`
- **THEN** it reads `status pages: 1 stored, 0 unchanged, 0 missing, 61 settled`
