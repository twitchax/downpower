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
| GovDelivery (`public.govdelivery.com/accounts/USNRC/subscriber/new`) | An "Event Reports" category. Its contents are unknown until something is subscribed. NRC's public bulletin feed (`updates.loc.gov/sitemap/USNRC.rss`) shows only ADAMS notices. The sign-up form carries an MTCaptcha, so a person subscribes. | Dropped 2026-11-16 (DESIGN.md Decision 39): never subscribed, and the `nrc@twitchax.com` route is deleted. |

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
- **No full-history bulk file.** `event-notification-rpt-lastmonth.txt` (pipe-delimited, 72
  columns, including `Cfr Cd1..4`, `Scram Code 1`, `Initial PWR 1` and `Event Text`) is live at
  `/sites/default/files/doc_library/reading-rm/doc-collections/event-status/event/`, beside
  `event-notification-rpt-datadictionary.xls`, but stale: last modified 2025-08-05 (checked
  2026-11-16), holding the 54 events updated in its last month (notified 2024-07-25 to
  2025-07-30). PRD-0003 uses it as a check on the page parser.
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
  - Match on the 10 CFR section, **never on the title**. Titles vary ("FITNESS FOR DUTY",
    "FITNESS-FOR-DUTY REPORT", "… PROGRAMATIC ISSUE"), and some ENs have no title at all.
  - Examples: EN 57379 (South Texas), EN 56939 (Limerick), EN 56963 (Sequoyah).
  - **About 905 events** in the stored archive (2026-11-15): 899 coded 26.73 (250), 26.719 (646)
    or 26.417(b)(1) (5), plus 6 from 1999–2008 coded 73.71. Power Reactor 880, Fuel Cycle 19, other
    categories 6.
  - Bare keywords misfire: "positive" matches 764 events, 202 not FFD (tritium, leaking sources);
    `THC` matches 10, all typos of "the"; "alcohol" catches isopropyl alcohol and the ATF;
    "contraband" catches weapons.
  - 22 to 44 a year. The code switches cleanly (last 26.73 on 2008-04-10, first 26.719 on
    2008-06-13), and the yearly average steps from 27.6 (1999–2007) to 35.4 (2009–2025).
  - Top states: GA 87, IL 71, PA 55, MI 52, NY 43. Vogtle alone has 72, 19 of them in 2018 during
    the Unit 3 and 4 construction.
  - Substances: only 426 events (47%) name one. Alcohol 408, marijuana or THC 6, opiates 5, cocaine
    4, amphetamines 3; generic wording only ("controlled substance") 209; nothing named 270.
  - Kinds: a person's test or violation 808, an item found in the protected area 74, a programmatic
    or lab failure 23. Of the 808, supervisors 527 (about 225 of them contractor supervisors),
    licensed operators 152, unspecified employees 60, contractors 46.
  - **Why it skews that way:** 10 CFR 26.719(b) makes an event of substances in a protected area,
    violations by licensed operators, supervisors and FFD program staff, and program integrity
    failures. Rank-and-file positives reach only the annual 26.717 summaries (PDFs for about
    CY1990–2016, with no state breakdown).
  - The 2008 Part 26 rewrite (73 FR 16966) took effect 2008-04-30, and licensees could defer parts
    to 2009-03-31.

### The stored archive (counted 2026-11-15)

Counted with a throwaway Python parser over the newest 200 of each of the 7,271 stored EN pages.

- **Size:** 20,171 event blocks, 19,725 distinct event numbers. 639 pages read "No events found".
  Each event is a `<div class="grid border" id="enNNNNN">` of `<b>Label:</b> value` pairs.
- **Pages are re-rendered, not archived.** A page shows the NRC's current database when fetched,
  so an old page already carries updates made years later. 404 events appear on more than one
  page, and 85 of those differ in text.
- **Categories:** Power Reactor (about 9,835), Agreement State 4,203, General Information or Other
  2,297, Fuel Cycle Facility 914, Other Nuclear Material 686, Non-Agreement State 622, Part 21
  492, Hospital 444, Non-Power Reactor 94, Research Reactor 58, General Information 37, ISFSI 29,
  Transportation 14. Research Reactor became Non-Power Reactor, and General Information became
  General Information or Other.
- **Two header templates.** Power Reactor and Fuel Cycle use Facility, Region, State, Unit, RX
  Type and Comments, with no license, docket or city. Every other category uses Rep Org, Licensee,
  Region, City, State, County, License #, Agreement and Docket.

  | Category | Events | License # | Docket | Distinct names (raw / normalized) |
  |---|---|---|---|---|
  | Agreement State | 4,203 | 93.6% | 0.2% | 3,238 / 2,608 |
  | General Information or Other | 2,297 | 76.7% | 0.9% | 1,833 / 1,647 |
  | Fuel Cycle Facility | 914 | none | none | 38 / 26 (Facility) |
  | Other Nuclear Material | 686 | 84.3% | 3.8% | 482 / 447 |
  | Non-Agreement State | 622 | 96.8% | 2.1% | 483 / 432 |
  | Part 21 | 492 | 0.6% | 2.2% | 255 / 195 |
  | Hospital | 444 | 95.3% | 1.6% | 355 / 336 |
  | Non-Power Reactor, Research Reactor | 94, 58 | 100% | 100% | 42 / 29, 23 raw |

  State is filled in on 98–100% of every category.
- **Names are messy.**
  - Case: 99% upper case through 2019, then title case (45% upper in 2020, 3–5% from 2021).
    Reactor facility names flip the same way (`VOGTLE`, `Vogtle`).
  - Rep Org is cut at 35 characters (1,379 values are exactly 35 long); Licensee isn't.
  - Newer pages use NUL bytes as empty placeholders.
  - Spellings vary: eight forms of "Mistras Group", three of "QSA Global".
  - (State, License #) makes 4,760 keys. 667 of them carry more than one normalized name (IL
    120072206 is "U S ARMY", "US ARMY TACOM" and "DEPARTMENT OF ARMY"), and a short number like
    `999` is only unique within its state.
  - Part 21's Licensee is often the affected plant, not the vendor.
  - Fuel cycle names follow ownership (B&W Nuclear Operating Group, then BWX Technologies;
    Framatome ANP Richland, then AREVA NP).
- **Appended updates.**
  - The canonical marker is `* * * UPDATE ON 09/28/2020 AT 1834 EDT FROM MATTHEW SENISON TO
    JEFFREY WHITED * * *`. Variants put AT before ON, use `@`, `BY X TO Y` or `FROM X TAKEN BY Y`,
    lead with the names, use 2-digit years, or use long star runs. FROM is the caller, TO the NRC
    duty officer, both free text.
  - 24.8% of events carry at least one marker (4,212 one, 506 two, 167 three or more). Last Update
    Date differs from the notification date on 21.9%.
  - Retractions: 5.7% carry a retraction marker, 7.0% counting retraction wording inside an update.
    By category: Power Reactor 8.2%, Hospital 10.8%, Agreement State 2.1%.
  - 1,201 events (6.1%) start with `EN Revision Imported Date: 9/29/2020` and `EN Revision Text:`,
    an NRC re-import around 2020–2023 rather than an update.
  - Over 6,070 markers, the date parses 97.3% of the time and is plausible 96.7% (the failures are
    typos like `17/14/05`), the time 94.0%, and from and to 94.4%. The regexes that measured this:

    ```text
    MARK = (?:\*\s*){3,}\s*(?P<kind>UPDATE|RETRACTION|RETRACTED|CORRECTION)\b(?P<rest>[^\n\r*]*)
    DATE = \b(\d{1,2})/(\d{1,2})/(\d{4}|\d{2})\b
    TIME = (?:\b(?:AT|@)\s*|\b)([0-2]\d[0-5]\d)\s*([ECMP][SD]T|[ECMP]T)?\b    # on rest, DATE removed
    PAIR = \b(?:FROM|BY)\s+(?P<frm>[^*@]+?)\s+(?:TO|TAKEN BY|RECEIVED BY)\s+(?P<to>[^*@]+?)(?=\s+(?:AT|ON|VIA)\b|\s*@|\s*$)
    ```
- **Edits after publication:** 22% of events are edited at all, 15.5% after day 2, 6.9% after
  day 28, 2.2% after day 90 (DESIGN.md Decision 29).
- **Status-report cross-references:** `See EN#` days carry 58 TWh, 2.4% of all lost output. A rough
  regex found 1,108 of the 1,488 referenced event numbers in the store; the misses include events
  from before 1999.


## NRC datasets

- **Scram dataset** (`/cdn/legacy/reactors/operating/dataset/scram-event-notifications.xlsx`):
  - Columns: `Year|Scram #|Plant Name|Docket|Site|PWR/BWR|Auto/Manual|Date|Event_Notification_Number|NRC_retract_flag|Link`.
  - 726 rows from 2011 to 2025-03, 37–69 a year. Last modified 2025-05-13 (checked 2026-11-15),
    so it isn't kept current. PRD-0003 reconciles our scram events against it by event number.
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
- **EIA-860 generator files, 1999 to 2006** (`archive/xls/eia860{YYYY}.zip` and
  `eia860a/eia860a{YYYY}.zip`): net summer capacity per generator before `NUC_STATUS` starts.
  - Three formats: 1999 and 2000 utility files (`ExistingGenerators{YYYY}.xls`, rated in **kW**,
    nuclear marked `UR` or `NUC`), `GENY{yy}.dbf` for 2001 to 2003, and `GenY{yy}.xls` for 2004 to
    2006 (both in MW).
  - All 104 operating units appear in every year, each matching a `NUC_STATUS` generator by plant
    and generator ID. The non-utility 860B files carry nameplate only and aren't needed.
  - Status `SD` in 1999 and 2000 marks a unit sold or moved to a non-utility owner (TMI 1, Pilgrim,
    Salem, Susquehanna); they kept operating. Browns Ferry 1 is `OS`, out of service, at 1,065 MW.
  - The largest 1999–2006 changes are uprates: Vermont Yankee +23%, Brunswick 2 +16%, Arkansas
    Nuclear 2 +15%. The fleet goes from 97.4 GW to 100.3 GW.
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

## Cannabis legalization dates (checked 2026-11-15)

For PRD-0005's overlay (DESIGN.md Decision 37).

- **Wikipedia, "Legalization of non-medical cannabis in the United States":** a table per state
  with "Effective date" and "Licensed sales since" columns. CC BY-SA, so a file built from it
  carries an attribution notice.
- **NCSL:** years only, for adult use and for comprehensive medical programs. The cross-check.
- **RAND OPTIC:** policy dates for 1990–2023, with no stated terms of use. Not used.
- **APIS** (NIAAA's alcohol policy system) answered 403.

## Third-party copies

- **`github.com/mike-welch/NRC_Power_Reactor_Status_Reports`:**
  - Monthly CSVs, 1999-01 to 2016-12 (216 files, about 29 MB), **with the reason column**:
    `Date,Unit,Power,Down,Reason or Comment,Change in report (*),Number of Scrams (#)`.
  - Scraped in 2017–19 with plain requests, before the block. `errors.log` lists missed days (for
    example, 1999-05-08 to 05-11).
  - No license file. The underlying NRC text is public domain.
  - Used only as a checker for our own parse (DESIGN.md Decision 14).
  - **Agreement** (`cargo xtask crosscheck`, 2026-11-14): of 677,769 unit-days both have, power
    agrees on 99.993%. Reasons agree on 99.922% of the 665,558 rows whose reason their unquoted CSV
    lets us recover, counting the 539 where theirs is a prefix of ours (their early rows cut reasons
    at about 75 characters). The remaining differences are on their side: `°` written as `?`,
    reasons blank or cut at a line break, and a few 2002 power values left blank.
  - Their early rows use the old reading-room names (`COOK 1`, `CALLAWAY 1`, `FT CALHOUN 1`, all
    upper case until mid-2003), so the check keys rows by docket.
  - **They have days the live site lost.** About 500 of their rows have no match here, nearly all
    on 2013-07-18, 2013-12-21 and 2015-12-02 to 12-04, which the live pages answer with "No data
    available". The old reading-room pages had them, so Wayback captures of those URLs may recover
    the five days, reasons included.
- **`zachlandes/nrc-event-scraper`:** daily EN JSONL, but its README says it fakes a Chrome TLS
  fingerprint and User-Agent to get past Akamai. **Off limits.**
- Copies without reason text, which add nothing: `theedge1620/NRC_event_notifications`,
  `BSVKey/nrc-reactor-feed`, `L0GIC0L/NPP-Tracker` and askthegrid.com (built on the 365-day file).
- Also blocked: INL's `lersearch.inl.gov` and `nrcoe.inl.gov` serve curl a Cloudflare challenge.
