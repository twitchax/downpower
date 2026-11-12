# Data sources: what exists, in what shape

From a probe on 2026-11-11. The counts come from samples, and each one says which. Treat this file as
the starting map, not as gospel: re-verify a source before building an ingester on it.

## The access problem (read this first)

**Live `www.nrc.gov` (and `ww2.nrc.gov`, `robots.txt` included) returns an Akamai 403 to every
non-browser client**: curl with an honest User-Agent, and fetch tools too. Everything NRC below was
read through Wayback Machine raw captures (`web.archive.org/web/<ts>id_/<url>`). Wayback throttles
hard: after a few dozen requests it serves empty bodies and "Temporarily Offline" pages, and some
captures are truncated. EIA is open to curl.

Spoofing a browser to get past the bot check is off the table. The legitimate routes:

- ask the NRC for bulk access, or for an allowlist
- use the EIA mirrors described below
- use Wayback at a polite rate
- check whether the 403 depends on the client IP (a home box versus a Fly machine)

This is open question 0 in `DESIGN.md`.

## NRC Power Reactor Status Reports (daily % power per unit)

- **Coverage:** daily from **1999-01-01** to today. There are 95 units now; Palisades returned after
  2024.
- **Snapshot time:** "Reactor status data collected between 4 a.m. and 8 a.m. each day. All times are
  based on eastern time." The 1999, 2008, 2016 and 2026 pages all carry this line.
- **Yearly raw text file, 2005 and later only:**
  - URL: `/reading-rm/doc-collections/event-status/reactor-status/{YYYY}/{YYYY}PowerStatus.txt`.
    The 2026 file is lowercase. There is also a rolling `PowerReactorStatusForLast365Days.txt`.
  - Pipe-delimited, `ReportDt|Unit|Power`, and **no reason text**.
  - 2024: 34,404 lines, 94 units, 1.3 MB.
  - The date format drifted from `12/31/2008` to `12/31/2024 12:00:00 AM`.
- **Daily HTML pages, 1999 and later.** These are the only source of the reason text.
  - Four tables, one per NRC region, with columns
    `Unit | Power | Down | Reason or Comment | Change in report (*) | Number of Scrams (#)`.
  - The same columns appear on 1999-01-04, 2008-07-15, 2016-07-27 and 2026-02-21.
  - **28-day embargo:** a page shows only `Unit | Power` until it is 28 days old, so the daily ingest
    has to go back and fill in the reasons.
  - About 9,800 pages in total.
  - **The site moved to Drupal around Sep 2026.** New pages live at
    `/documents-reports/document-collections/events-reports-associated-with/power-reactor-status-reports/YYYY/YYYYMMDDps`,
    with no `.html`.
- **Reason vocabulary is small** (sample: July 2025, 19 of 31 days):
  - 1,786 unit-days, 274 with a reason, **54 distinct strings**.
  - Strings join several tags with `;`, so there are only about **30 atomic tags**.
  - Classification can be rules-first; an LLM may never be needed.
  - Examples:
    - Heat and environment: `Hot Weather Alert`, `Heat Advisory`, `Reduced Power Due to Environmental Conditions`
      (Diablo Canyon 2 at 55%).
    - Fuel cycle: `Coastdown to Refueling Outage`, `Refueling Outage`.
    - Equipment: `Transformer Limitation`, `Steam Generator Limitations`,
      `Main Condenser Inefficiencies`, `Single Loop Ops due to Recirculation Pump Trip`, `Forced Outage`,
      `Reduced Power for Rod Pattern Adjustment`.
    - Grid: `Maximum Generation Alert`, `Yellow Grid Status`, `Hands Off Grid Restriction`,
      `Limited to Less Than 1250 MWe. 300 MVAR Minimum.`
    - Cross-references: `See EN# 57813`, `See EN57813`, `See Event Number 57813`.
- **Quality gotchas:**
  - "Environmental Conditions" doesn't say whether the cause is heat, kelp or swell.
  - Grid tags also appear on rows at 100% power.
  - Case is inconsistent.
  - Blank cells contain **NUL (`\x00`) bytes**.
  - The `Down` date is MM/DD/YYYY through 2016 and ISO from 2025.
  - Unit names are all caps in 1999 and differ between NRC, EIA and the Info Digest
    (`Susquehanna 1` / `PPL Susquehanna`, `ANO 1`).

## NRC Event Notification Reports (ENs)

- **Coverage:** daily pages from **1999**.
- **No full-history bulk file.** `event-notification-rpt-lastmonth.txt` (pipe-delimited, about 74
  columns, including `Cfr Cd1..4`, `Scram Code 1`, `Initial PWR 1` and `Event Text`) covers one month
  only; the last capture is from 2019-11, and whether it is still live is unknown.
- **Structure (2024):**
  - Structured fields: category header (Power Reactor, Agreement State, Part 21, …), Event Number,
    Facility, Region, State, Unit, RX Type, Notified By, HQ Ops Officer, Notification Date/Time (ET),
    Event Date/Time (local), Last Update Date, Emergency Class, one or more `10 CFR Section` (code plus
    description), and Person (Organization).
  - Power reactors also get a unit table: `Unit|SCRAM Code|RX Crit|Initial PWR|Initial RX Mode|Current PWR|Current RX Mode`.
  - Free text: an all-caps title, the body, and appended `* * * UPDATE … * * *` and retraction blocks.
- **The format changed twice:**
  - 2000: fixed-width ASCII in `<pre>`.
  - 2010: HTML tables.
  - 2024: mixed-case `label: value` lines.
  - The exact cutover dates are unknown.
- **Pages are not append-only.** Updates re-post old ENs, so deduplicate by EN number and key on the
  notification date. CFR text sometimes wraps across lines.
- **Fitness for duty:**
  - Match on `10 CFR Section: 26.719 - Fitness For Duty`, **never on the title**. Titles vary
    ("FITNESS FOR DUTY", "FITNESS-FOR-DUTY REPORT", "… PROGRAMATIC ISSUE"), and some ENs have no title
    at all.
  - Examples: EN 57379 (South Texas), EN 56939 (Limerick), EN 56963 (Sequoyah).
  - Volume: Q1 2024 had 8 FFD ENs out of 134 unique, which points to roughly **35–45 a year**. That is an
    extrapolation, not a full-year count.

## NRC datasets

- **Scram dataset** (`/cdn/legacy/reactors/operating/dataset/scram-event-notifications.xlsx`):
  - Columns: `Year|Scram #|Plant Name|Docket|Site|PWR/BWR|Auto/Manual|Date|Event_Notification_Number|NRC_retract_flag|Link`.
  - 726 rows from 2011 to 2025-03, 37–69 a year.
  - Use it as a cross-check, never as a substitute.
- **Operating reactors** (`/reading-rm/doc-collections/datasets/reactors-operating.xlsx`; the capture
  is the 2022 update):
  - Covers docket, license, location, region, reactor and containment type, vendor,
    architect-engineer, licensed MWt, MWe, and capacity factors.
  - Dates are Excel serial numbers. The current live URL is unknown.

## EIA (open to curl)

- **`nuclear-outages/generator-nuclear-outages` API:**
  - **Daily from 2007-01-01 to today.** Gives capacity and outage MW per EIA plant and generator,
    *sourced from the NRC status report*.
  - Works as the **bridge from NRC unit names to EIA plant and generator IDs**.
  - It also gives a numeric backbone that needs no nrc.gov access at all. Example: Diablo Canyon 2 has
    a 45% outage on 2025-07-05, matching the NRC report's 55% "Environmental Conditions" line.
- **EIA-860 Schedule 6D, "Cooling"** (sheet in `6_2_EnviroEquip_Y{YYYY}.xlsx`):
  - 2012 and later, plus a 2008 file.
  - Cooling type for all 54 nuclear plants in 2024: `ON` is once-through; `RI`/`RN` are induced- and
    natural-draft towers. Examples: Diablo Canyon `ON`, Byron `RN`, Browns Ferry `ON`+`OT`.
- **EIA-923 Schedule 8D:**
  - **Monthly** intake and discharge water temperatures (average and max, °F) by plant and cooling
    ID, 2008 and later.
  - 2024: 54 of 54 nuclear plants, 724 of 828 rows with an intake temperature. Missing values are
    `.`, and Palo Verde has none.
  - Example, July 2024: Diablo Canyon 53/55 in and 73/75 out; Browns Ferry 87/90 in.
  - The separate "Thermoelectric cooling water data" files have *no* temperatures.
- **EIA-923 monthly generation:**
  - Plant level, fuel `NUC`; the API (`electricity/facility-fuel`) covers 2001-01 to now.
  - Watch for duplicate `ALL` and `ST` prime-mover rows, which double-count.
  - Use it to measure the energy the 4–8am snapshot misses.
- **EIA-930:**
  - "Net Generation (MW) from Nuclear", **hourly** by balancing area, from 2018-07-01 in the CSVs
    (the API starts 2019-01-01).
  - It shows the afternoon derates the morning snapshot misses, but only for balancing areas with few
    plants.
- **Spreadsheet gotcha:** EIA xlsx files put several title rows above the real header.
