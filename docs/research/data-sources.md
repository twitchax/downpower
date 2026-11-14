# Data sources: what exists, in what shape

From a probe on 2026-11-11, plus the Wayback coverage counts and the survey of other channels on
2026-11-12. The counts come from samples, and each one says which. Treat this file as the starting
map, not as gospel: re-verify a source before building an ingester on it.

## The access problem (read this first)

**As of 2026-11-12, live `www.nrc.gov` answers plain clients again.** Through 2026-11-11 it (and
`ww2.nrc.gov`, `robots.txt` included) returned an Akamai 403 to every non-browser client, so the
2026-11-11 probe read everything NRC through Wayback raw captures
(`web.archive.org/web/<ts>id_/<url>`). On 2026-11-12, curl with the honest User-Agent
`downpower-research/0.1 (+https://github.com/twitchax/downpower)`, pacing requests 5 s apart, got a
200 from **both a Fly machine in `sjc` and the home box** for:

- `robots.txt`
- status pages for 1999-01-05, 2024-07-02, 2026-10-14 and 2026-11-11
- Event Notification pages for 1999-01-05, 2024-07-02 and 2026-11-11
- `PowerReactorStatusForLast365Days.txt`

Every page came from the new Drupal 11 site, and each old `/reading-rm/doc-collections/event-status/...html`
URL returned a 301 to its new path. Whether the 403 ended with the migration or with an Akamai rule
change is unknown, and it may come back: a fetcher should treat a 403 as "stop and alert", never as
"retry harder". Spoofing a browser stays off the table if it does.

`robots.txt` (2026-11-12) gives `User-agent: *` a `Disallow` only for `ads.txt`, autodiscover and
site search, with no `Crawl-delay`. `Amazonbot` and `Akamai-SiteSnapshot` are kept out of `/docs/`.

The Wayback crawler was blocked too while the 403 lasted: its 2026-04-10 capture of `www.nrc.gov/data/`
is itself a stored 403, which is probably why its coverage drops in 2025–26 (below). Wayback also
throttles hard: after a few dozen requests it serves empty bodies and "Temporarily Offline" pages, and
some captures are truncated. EIA is open to curl. DESIGN.md Decisions 10–13 were settled while the
block stood, and are reopened ("Still open" there).

### Other channels, checked 2026-11-12

None of them carries the reason text. They were surveyed while the 403 stood.

| Channel | What it has | Usable? |
|---|---|---|
| NRC RSS: `public-involve/rss?feed=plant-status`, `?feed=event` | Titles only: "Unit - N% power", "EN number - organization". No dates, no reasons, no event text. | No: titles only. |
| data.gov: "U.S. Nuclear Power Reactor Plant Status", "Event Reports for Operating Reactors" | Distribution URLs point to `PowerReactorStatusForLast365Days.txt` (no reasons) and `event-notification-rpt-lastmonth.txt`. CC0. | Numbers only. The 365-day file is live (see below). |
| ADAMS API (`adams-api.nrc.gov`, Azure) | Not behind Akamai, but needs a free subscription key (`Ocp-Apim-Subscription-Key`). Document links point to `www.nrc.gov/docs/...`. Only scattered EN printouts were found, not a series. | No. |
| GovDelivery (`public.govdelivery.com/accounts/USNRC/subscriber/new`) | An "Event Reports" category. Its contents are unknown until something is subscribed. NRC's public bulletin feed (`updates.loc.gov/sitemap/USNRC.rss`) shows only ADAMS notices. The sign-up form carries an MTCaptcha, so a person subscribes. | Maybe. `nrc@twitchax.com` (Cloudflare Email Routing, live 2026-11-12) is the test address. |

**Contacts:**
- Web Team: `nrc.gov/site-help/feedback`.
- Chief Data Officer: Basia Sall, `Basia.Sall@nrc.gov`.
- Statistical Official: Shaun Anderson, `Shaun.Anderson@nrc.gov`.
- Public Document Room: `PDR.Resource@nrc.gov`.
- FOIA: `FOIA.Resource@nrc.gov`.

The data.gov contact (`Adam.Glazer@nrc.gov`) dates from 2010.

## NRC Power Reactor Status Reports (daily % power per unit)

- **Coverage:** daily from **1999-01-01** to today. There are 95 units now; Palisades returned after
  2024.
- **Snapshot time:** "Reactor status data collected between 4 a.m. and 8 a.m. each day. All times are
  based on eastern time." The 1999, 2008, 2016 and 2026 pages all carry this line.
- **Yearly raw text file, 2005 and later only:**
  - URL: `/reading-rm/doc-collections/event-status/reactor-status/{YYYY}/{YYYY}PowerStatus.txt`.
    The 2026 file is lowercase. There is also a rolling `PowerReactorStatusForLast365Days.txt`.
  - Pipe-delimited, `ReportDt|Unit|Power`, and **no reason text**.
  - The NRC Data Dictionary (`dd.xls`, linked from each yearly index) defines Power only as "Power
    Reactor Operating Capacity for that date (between 0 and 100 percent)". It doesn't say thermal or
    electrical. The evidence points to thermal: Clinton at 99% with the comment "100% ELECTRICAL
    CAPABILITY", and `Limited to … 1250 MWe` grid caps on rows below 100%.
  - 2024: 34,404 lines, 94 units, 1.3 MB.
  - The date format drifted from `12/31/2008` to `12/31/2024 12:00:00 AM`.
- **Daily HTML pages, 1999 and later.** These are the only source of the reason text.
  - Four tables, one per NRC region, with columns
    `Unit | Power | Down | Reason or Comment | Change in report (*) | Number of Scrams (#)`.
  - The same columns appear on 1999-01-04, 2008-07-15, 2016-07-27 and 2026-02-21.
  - **28-day embargo:** a page shows only `Unit | Power` until it is 28 days old, so the daily ingest
    has to go back and fill in the reasons.
  - 10,179 pages from 1999-01-01 through 2026-11-13.
  - **Wayback coverage** (CDX, 2026-11-12): to have the reasons, a capture must be taken 28 or more
    days after its report date.

    | Years | Report days with such a capture |
    |---|---|
    | 1999–2024 | all of them |
    | 2025 | 289 of 365 (79%) |
    | 2026, through 10-14 | 45 of 287 (16%) |
    | Total | 9,830 of 10,149 |

    This counts captures that exist; truncated bodies only show up on fetch. The new Drupal path has
    only 9 captures in all.
  - **The site moved to Drupal around Sep 2026.** New pages live at
    `/documents-reports/document-collections/events-reports-associated-with/power-reactor-status-reports/YYYY/YYYYMMDDps`,
    with no `.html`, and old URLs 301 there.
    - The Drupal site re-renders old years too. Live 1999-01-05 and 2026-10-14 pages share one header
      row, `Unit | Power | Down | Reason or Comment | Change in Report (*) | Scrams (#)`, and the 1999
      page's `Down` dates are ISO (`1996-02-20`). The format drift listed under "Quality gotchas"
      describes Wayback's captures of the old site, not live pages.
    - The 28-day hold still applies: on 2026-11-12, the 2026-11-11 page had only `Unit | Power`, and
      the 2026-10-14 page had every column (25 rows below 100%, all with a reason).
    - `PowerReactorStatusForLast365Days.txt` moved to `.../power-reactor-status-reports/PowerReactorStatusForLast365Days.txt`
      (34,770 lines on 2026-11-12, `ReportDt|Unit|Power`, newest day first).
    - The Data Dictionary is at
      `/sites/default/files/doc_library/reading-rm/doc-collections/event-status/reactor-status/data_gov_powerstatus_datadictionary.xls`.
    - **The 2026-11-13 backfill found one template on every page.** All 10,179 status pages from
      1999-01-01 have the four region tables and the header above. Power is always an integer from
      0 to 100, `Down` is ISO or blank, `Change in Report` is `*` or blank, and `Scrams` is blank or
      `1` (first on 2013-06-29). Details: `.prds/PRD-0002.log.md`, T-003.
    - **A missing day answers 200, not 404,** with "No data available for this date." and no tables.
      1999-05-08, 2013-07-18, 2013-12-21 and 2015-12-02 to 12-04 read this way on two fetches a day
      apart, so they are permanent gaps. A day whose page isn't published yet reads the same.
    - Pages from 2025 on put a lone NUL byte in a blank reason cell, and join tags with `;` and CRLFs.
- **Reason vocabulary is small** (sample: July 2025, 19 of 31 days):
  - 1,786 unit-days, 274 with a reason, **54 distinct strings**.
  - Strings join several tags with `;`, so there are only about **30 atomic tags**.
  - Classification can be rules-first; an LLM may never be needed.
  - **Every unit-day below 100% had a reason** in the sample (150 of 150).
  - **The grid and weather tags are mostly context, not causes.** In the sample, they sit mostly on
    rows at 100% power:

    | Tag | Rows at 100% |
    |---|---|
    | Hot Weather Alert | 38 of 43 |
    | Maximum Generation Alert | 59 of 68 |
    | Conservative Operations / Ops | 29 of 33 |
    | Yellow Grid Status / Condition | 15 of 16 |
    | Heat Advisory | 2 of 2 |

    They are `flag`s in DESIGN.md Decision 18.
  - Examples:
    - Heat and environment: `Reduced Power Due to Environmental Conditions` (Diablo Canyon 2 at 55%),
      and the `Hot Weather Alert` and `Heat Advisory` flags.
    - Transitions: `Increasing Power`, `Startup in Progress`, `Holding Power`.
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

- **Coverage:** daily pages from **1999**, **business days only**. Weekend events land on the next
  business day's page.
- **Wayback coverage** (CDX, 2026-11-12):
  - 1999–2024: 6,504 of 6,783 weekdays. The 279 missing (about 11 a year) are federal holidays.
  - 2025: 230 pages, about 92% of business days.
  - 2026: 48 pages.
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
  - These are the eras of Wayback's captures. The live Drupal site re-renders old pages: its
    1999-01-05 page carries `label: value` fields (`Event Number: 35219 Facility: MILLSTONE ...`) and
    the unit table header `Unit | SCRAM Code | RX Crit | Initial PWR | Initial RX Mode | Current PWR`.
    New path: `.../event-notification-reports/YYYY/YYYYMMDDen`.
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
  - **The v2 API needs a key, even for metadata:** a keyless request gets a 403 `API_KEY_MISSING`.
- **Keyless bulk file: `https://www.eia.gov/opendata/bulk/NUC_STATUS.zip`** (listed in
  `opendata/bulk/manifest.txt`). This is the one downpower uses (DESIGN.md Decision 11).
  - 10.8 MB zipped, 57.8 MB of NDJSON.
  - 522 daily series at US, plant (66) and generator (107) level. Each level has capacity MW, outage
    MW and percent outage.
  - Series IDs look like `NUC_STATUS.OUT_PCT.6014-1.D`: EIA plant ID, then generator.
  - Data from 2007-01-01 with about a one-day lag. Weekend and holiday data arrive the next weekday.
  - **It is the NRC power column, transformed.** Percent outage, always a whole number, equals 100
    minus NRC power on all but 12 of the 714,972 in-service unit-days both report from 2007 to
    2026-11-13 (`cargo xtask eia`; eight of the 12 are the duplicated 2013-07-19 page).
  - It fills the NRC's five "No data" days since 2007 (2013-07-18, 2013-12-21, 2015-12-02 to
    12-04). It lacks 1,556 NRC unit-days: the partial month before each retirement, River Bend 1
    from 2013 to 2019-01-23, and holes in Vogtle 3 and 4 during startup testing.
  - `OUT` is `CAP` × `OUT_PCT` / 100 on every day, and the median generator's capacity changes 41
    times since 2007.
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

## Third-party copies

- **`github.com/mike-welch/NRC_Power_Reactor_Status_Reports`:**
  - Monthly CSVs, 1999-01 to 2016-12 (216 files, about 29 MB), **with the reason column**:
    `Date,Unit,Power,Down,Reason or Comment,Change in report (*),Number of Scrams (#)`.
  - Scraped in 2017–19 with plain requests, before the block. `errors.log` lists missed days (for
    example, 1999-05-08 to 05-11).
  - No license file. The underlying NRC text is public domain.
  - Used only as a checker for our own parse (DESIGN.md Decision 14).
- **`zachlandes/nrc-event-scraper`:** daily EN JSONL, but its README says it fakes a Chrome TLS
  fingerprint and User-Agent to get past Akamai. **Off limits.**
- Copies without reason text, which add nothing: `theedge1620/NRC_event_notifications`,
  `BSVKey/nrc-reactor-feed`, `L0GIC0L/NPP-Tracker` and askthegrid.com (built on the 365-day file).
- Also blocked: INL's `lersearch.inl.gov` and `nrcoe.inl.gov` serve curl a Cloudflare challenge.
