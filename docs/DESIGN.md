# downpower: design notes

**Status:** decisions settled through grilling round 3 (2026-11-12). The first milestone is
`.prds/PRD-0002.md`. What is still open, and what each open item waits on, is under "Still open".

## Intent

**What.** A public, visual-first analysis of why US nuclear reactors lose output, broken down by cause
(heat and environmental derates, forced outages, refueling, end-of-cycle coastdowns, grid and
load-following, scrams). Alongside it sit the NRC Event Notifications: scrams, fitness-for-duty (FFD),
security, ESF actuations, and Part 21 defects.

**Why it is worth doing.** The heat-derate question has prior art, but all of it rests on data that is
plant-level, blind to cause, sampled once a day in the early morning, and ends in 2018. The NRC's daily
Power Reactor Status Reports carry a free-text reason for every unit on every day. Nobody has classified
those reasons at scale, and that classification is how downpower gets in.

**Output.** Visuals first. A paper follows only if the findings are novel.

## Prior art, and what it leaves open

| Work | What it did | What it left open |
|---|---|---|
| [Coffel & Mankin 2021, *ERL*](https://iopscience.iop.org/article/10.1088/1748-9326/abd4a8) | 61 US nuclear plants, 2007–2018, using EIA daily available capacity (derived from NRC), July–August only. Daily max dry-bulb air temperature plus runoff, with cooling type as a fixed effect. Thermal plants overall: +0.8–1.2 pp curtailment per °C at peak. | No cause labels: forced outages, coastdowns and load-following are counted as heat. Plant-level, not unit-level. No water temperature and no wet-bulb. No separation of permit or tech-spec limits from efficiency loss. Ends in 2018. |
| [Linnerud et al. 2011](https://ideas.repec.org/a/aen/journl/2011v32-01-a06.html) | European nuclear: about 0.5% output lost per °C from efficiency, and more than 2%/°C in heat waves and droughts. | Europe only. |
| [Ahmad 2021, *Nature Energy*](https://link.springer.com/article/10.1038/s41560-021-00849-y) | Global IAEA outage events: climate-linked outages went from 0.2 to 1.5 per reactor-year (1990s → 2010s). | Counts outage events, not lost MWh. |
| [INL LWRS 2023](https://inldigitallibrary.inl.gov/sites/sti/sti/Sort_66638.pdf), [NREL](https://www.nrel.gov/docs/fy17osti/66714.pdf), [GAO-24-106326](https://www.gao.gov/assets/gao-24-106326.pdf) | Incident catalogs and qualitative risk work. Once-through plants in the southern Midwest and Southeast are the most exposed. | Qualitative, or case by case. |
| [NRC scram dataset + dashboard](https://www.nrc.gov/facilities-safety/operating-reactors/reactor-safety-information-topics/operating-reactor-scram-trending) | The NRC's own scram counts since 2011. | Already done, so **do not rebuild a scram tracker.** Use it as a cross-check. |

On FFD, we found nothing beyond the NRC's aggregate annual FFD summaries and a
[2017 UCS post](https://blog.ucs.org/dlochbaum/marijuana-and-nuclear-power-plants/). A breakdown by state
against legalization dates looks new.

## Decisions (settled 2026-11-11)

1. **Name: `downpower`.** It is the industry's word for a power reduction and covers every cause.
   - `cherenkov` was rejected because it doesn't relate to the thing being analyzed.
   - `baseload` was rejected as unsearchable.
   - `derate`, `coastdown` and `heatsink` were rejected because each names a single cause.
2. **Audience: a public site on Fly from week 1.** It stays effectively private until an announcement.
   Every chart state is permalinkable from the start.
3. **Model: two fact tables on a shared plant, unit and state spine.**
   - `unit_day` holds power %, lost MWh and the classified cause.
   - `event` holds every Event Notification.
   - FFD lives in `event`, not in the loss ledger, because a positive test almost never costs output.
   - Scrams appear in both tables, which gives a free cross-check between the two NRC sources.
4. **All Rust, statistics included.** Revisit only if the paper conversation happens. Staggered
   difference-in-differences for legalization is where hand-rolled estimators get attacked;
   [Callaway & Sant'Anna](https://bcallaway11.github.io/did/) is the reference implementation, and plain
   TWFE is biased under staggered adoption.
5. **Explorer first, plus a findings layer.** Each finding is a saved, permalinked chart state with a
   note. The findings become the outline for the post or paper.
6. **Look: the instrument design language without the grid background.** That keeps it from reading like
   a work tool. The body no longer carries `.grid-bg`.
7. **Freshness: one idempotent, incremental pipeline (raw cache → parse → normalize)** serves both the
   backfill and the daily update. The daily update runs as a background task inside the server binary.
   Every raw page is kept, because the NRC has moved URL schemes before (`reading-rm/...` →
   `documents-reports/...`).
8. **Repo: public, at `github.com/twitchax/downpower`.**
9. **Store: SurrealDB 4, embedded (`kv-surrealkv`) on the Fly volume.** `DOWNPOWER_DATA_DIR` points at
   it, and the default is `./data`.

## Decisions (settled 2026-11-12)

Grilling rounds 2 and 3. The facts behind them, including the Wayback coverage counts, are in
`docs/research/data-sources.md`.

### Sources and access

Decisions 10, 12 and 13 were revised later on 2026-11-12, after the nrc.gov 403 lifted
(`data-sources.md`, "The access problem"). The Wayback plan they replaced survives as the fallback.

10. **Pages come from nrc.gov directly.** Every status page from 1999 on, and every Event
    Notification page, is fetched once into the raw store (Decision 15).
    - Politeness: one request every 5 seconds under `downpower/0.1 (+https://github.com/twitchax/downpower)`,
      the User-Agent the NRC email names, with backoff on 429s and 5xx responses.
    - A page is settled once it has been fetched far enough past its date: 28 days for a status page
      (the reason column fills in), 2 days for an EN page. The fetcher resumes by skipping settled
      pages.
    - **A 403 stops the run and alerts.** If the block returns, the fallback is the original plan: for
      each report date, the newest Wayback capture taken 28 or more days after it.
    - About 10,200 status pages (roughly 14 hours) and 7,000 EN pages (roughly 10 hours), run once
      from the home box.
    - The raw NRC page stays the record; no third-party copy is taken outright.
11. **Numbers come from EIA's keyless bulk `NUC_STATUS.zip`.** It covers 2007 onward, about a day
    behind, and supplies capacity and the days the NRC pages miss.
    - It needs no API key; the v2 API does, even for metadata.
    - Where an NRC page and EIA both exist, the NRC page wins.
    - EIA's percent outage is 100 minus the NRC power value, so their disagreement rate (shown on
      `/status`) mostly catches crosswalk mistakes. Across 2007–2026 they disagree on 12 of 714,972
      unit-days.
    - The raw store keeps the NDJSON file inside the zip, about 6 MB as a blob.
12. **Recent reason text comes from re-fetching.** The daily job fetches the newest status page
    (numbers only) and the page that has just passed its 28-day hold (reasons). Until a day passes
    the hold, its cause reads "reason text not yet available".
13. **The NRC email.** Aaron sent it to the Chief Data Officer on 2026-11-12.
    - It asks what the Power column measures (see "Still open") and whether a bulk export of both
      series exists.
    - It tells the NRC that, absent an export, downpower reads the archive once at the rate and
      User-Agent in Decision 10.
    - Its first draft asked for an allowlist. That ask was dropped when the 403 lifted.
    - Rejected while the block stood: Wayback's Save Page Now used as a proxy, which just routes our
      requests around the block, and data from third-party scrapers that fake a browser fingerprint.
14. **The `mike-welch/NRC_Power_Reactor_Status_Reports` CSVs (1999–2016) are a checker, not a
    source.** We compare our parse against them once and record the agreement rate in the docs. The
    CSVs are not vendored: their repo has no license file.

### Storage

15. **Raw pages are the system of record.** Each fetched page is stored zstd-compressed and named by
    its sha256, plus an append-only manifest (source, URL, capture time, fetch time, hash).
    - Surreal holds everything derived, analysis outputs included.
    - A `derived_version` stamp triggers a background rebuild from raw whenever the parser or the tag
      table changes. Each build is its own table, and the previous one keeps serving until the new
      one is complete (revised 2026-11-14).
    - The backfill runs on the home box. The raw files ship to the volume, and the database is
      rebuilt there by the deployed code.
    - Backup: Fly's daily volume snapshots plus the home-box copy.
16. **The unit key is the NRC docket number** (for example, `05000275` is Diablo Canyon 1).
    - A committed crosswalk CSV maps each docket to every NRC name variant, the EIA plant and
      generator, a display name, and the retirement date.
    - It covers units retired since 1999.
    - A unit is in service from commercial operation (Watts Bar 2, Vogtle 3 and 4 started after
      1999), and out of service between a shutdown and a restart, such as Browns Ferry 1 from 1985
      to 2007. The pages list a unit in startup testing or a defueled unit at low power, and
      neither is lost output (revised 2026-11-14).
    - A name the crosswalk doesn't know fails the backfill.

### Classification

17. **Causes come in two levels: fine causes grouped into 8 families.** The families are the
    barcode's colors.

    | Family | Includes |
    |---|---|
    | Refueling | refueling outages |
    | Fuel cycle | coastdown, rod pattern adjustment |
    | Forced | scrams, forced outages, trips |
    | Maintenance | planned maintenance |
    | Equipment limitation | steam generator, transformer, condenser, recirculation pump |
    | Environmental | heat, water, marine life, weather |
    | Grid | MWe/MVAR limits, dispatch |
    | Unattributed | unknown tags, unresolved EN references, everything else |

    `Main Condenser Inefficiencies` sits under Equipment, but its fine cause keeps it addressable for
    the heat analysis.
18. **Each tag-table entry has a kind.**
    - `cause` attributes lost output.
    - `flag` is context only: grid and weather alerts, conservative ops. Flags are stored as a set on
      `unit_day`. In the July 2025 sample they mostly sit on full-power rows; for example, Hot Weather
      Alert is on 38 of 43.
    - `transition` covers Increasing Power, Startup in Progress, Holding Power and Defueled. A
      transition continues the unit's last attributed cause when that day is at most 14 days back,
      and is Unattributed otherwise (revised 2026-11-14). Inherited days count as attributed, so a
      run of transitions (weeks of "DEFUELED" after one refueling tag) carries the cause through,
      and the evidence names the day the cause was observed. A newly observed cause restarts the
      chain, and a day without a cause neither extends nor breaks it.
    - `xref` covers `See EN# N`. It resolves through `event` once Event Notifications land, and until
      then counts as Unattributed with the EN number kept.
    - Matching (revised 2026-11-13): normalize (lowercase, collapse whitespace), split the reason
      into tags on `;` and line breaks, and look up each tag's exact key. A tag no key matches goes
      through an ordered list of keyword rules, each a committed regex mapped to one entry, and the
      first rule that matches wins. The observed layer records which key or rule matched. There is
      still no fuzzy matching (no edit distance, no embeddings).
    - Rules, because exact keys can't cover the archive: it holds 21,864 distinct tags, 12,212 of
      them used once, and before about 2019 the column is free text. The top 1,000 keys cover 81%
      of tag uses, and eleven rough keyword rules already cover 87%.
19. **A precedence order picks the primary cause on multi-cause days:** Forced > Refueling >
    Maintenance > Equipment > Environmental > Grid > Fuel cycle > Unattributed.
    - Every cause stays on the row, so views can count "any mention".
    - Splitting the loss between causes would imply a precision the text doesn't have.
    - The heat analysis drops any day that carries a fuel-cycle tag.
20. **Every label is either observed or attributed.**
    - *Observed:* the tags as written and the causes the table maps them to. It's deterministic and
      checkable against the source text.
    - *Attributed:* everything inferred, meaning the primary cause (Decision 19), transition
      inheritance, resolved EN references, and the heat-versus-marine split of "Environmental
      Conditions". Each attributed value records the method that produced it (for example
      `precedence_v1`, `inherit_14d`) and its evidence.
    - "Environmental Conditions" is observed as `environmental_unspecified` and is never heat by
      default.
    - Charts default to the attributed layer and can be switched to observed only.
21. **Unknown tags fail at three levels** (revised 2026-11-13). A tag is *known* when a key or rule
    matches it, or when the committed `catalog/unclassified.tsv` lists it, which accepts it on
    purpose as Unattributed.
    - A CI test checks that every distinct tag in the committed `catalog/reasons.tsv` snapshot of
      the archive is known. `cargo xtask reasons` regenerates the snapshot from the raw store.
    - The backfill and an operator's rebuild fail hard on a tag that isn't known.
    - The daily ingest and the server's own background rebuild store the row as `unknown_tag`
      (counted as Unattributed) and list it on `/status`, so one new tag can't freeze the site
      (revised 2026-11-14: the server's rebuild runs unattended after a deploy).
    - An LLM only proposes new table entries, offline.
22. **Lost output = (100 − power) % × net capacity × 24 h, labeled "reported power reduction".**
    - Capacity comes from `NUC_STATUS` from 2007 on, and from EIA-860 net summer capacity before
      that (`catalog/capacity.csv`). A zero `NUC_STATUS` rating isn't one: EIA zeroes a retiring
      unit's capacity weeks before it stops.
    - "100% electrical capability" and its variants become their own cause,
      `full_electrical_output`, which books no lost output (`no_lost_output` in `catalog/tags.toml`,
      revised 2026-11-14). A unit at full electrical output reading 97–99% thermal has lost no
      energy against its net capacity. Turbine, administrative and uprate limits still count.
    - The heat efficiency loss ("the slope") probably never appears in the NRC column (see "Still
      open"). It is measured separately, as the monthly gap between actual EIA-923 generation and
      the generation implied by reported power.

### Views

23. **Charts are hand-rolled.**
    - SVG views keep their geometry in a pure, unit-tested module, and the view only places it.
    - The barcode is the one canvas view. It draws from a packed buffer with one byte per cell (3
      bits family, 5 bits power bucket), about 1.1 MB before gzip, served by a dedicated ETag-cached
      endpoint.
    - Chart state lives in the URL query, parsed into one typed struct per view.
    - Load the `dataviz` skill before building the first chart.
24. **The hero view is the fleet barcode,** with the stacked area of lost MWh by cause beneath it as
    its summary. Both share one filter state. It may be revisited.
25. **Findings are markdown files in git, `findings/NNNN-slug.md`.**
    - The frontmatter holds the view and its URL state, and the body holds the note.
    - They are loaded into Surreal on every rebuild so they can be queried. They live in git, not
      Surreal, because they can't be rebuilt from raw, deserve review, and can be checked in CI: a
      test asserts every finding's state still parses.
    - A "save as finding" button, present only in dev mode, writes the file.
26. **Sequence.**
    - **PRD-0002:** the status-report ledger as a deployed vertical slice.
    - **PRD-0003:** Event Notifications. Their raw pages are fetched alongside the status pages
      (Decision 10), so this means the EN parser, EN reference resolution, the scram cross-check,
      the FFD view, and the email channel if the NRC list carries full text.
    - **Then the joins, in order:** EIA-860 cooling type and water body, then NOAA hourly weather
      (wet-bulb), then EIA-923 generation, then EIA-923 Schedule 8D, then USGS intake temperature,
      then EIA-930.
27. **Domain: `downpower.twitchax.com`.** DNS is on Cloudflare. A domain of its own only if the
    announcement earns one.

## Still open

- **The daily Event Notification channel.** The live EN page may be enough by itself. The NRC's
  "Event Reports" email list gets a test subscription from `nrc@twitchax.com`, a Cloudflare Email
  Routing address (live 2026-11-12) that forwards to Aaron's inbox.
- **What the Power column measures.** Waiting on the NRC's reply (Decision 13). Its Data
  Dictionary says only "Power Reactor Operating Capacity for that date (between 0 and 100
  percent)". The evidence points to percent of rated *thermal* power: a 99% unit annotated "100%
  ELECTRICAL CAPABILITY", and `Limited to … 1250 MWe` grid caps appearing below 100%. Decision 22
  doesn't depend on the answer, but the "slope versus cliff" analysis does.
- **FFD analysis design** (the staggered difference-in-differences estimator, `BREAKS.md`). This
  comes after PRD-0003.

## Deploy

- `fly.toml` is at the root.
  - App `twitchax-downpower` (created 2026-11-12), region `sjc`. Fly deprecated `sea`, and it refuses
    new resources there.
  - Always on (`min_machines_running = 1`, no auto-stop), so the ingest task never skips a day.
  - Health check on `/healthz`.
- **Gotcha: volume ownership.** Fly mounts volumes root-owned, but the image runs as uid 10001, so the
  first boot's `db::open` will fail with permission denied. The recommended fix is an entrypoint that
  runs as root, chowns `/data`, and then drops to uid 10001 with `setpriv` (util-linux ships in
  `debian:stable-slim`). It isn't done in the scaffold because only a real Fly volume can test it.
- **Gotcha: SurrealKV sizes its memory from the host.**
  - With the defaults, the smoke run on a 256 GB box logged a 4 GiB max memtable and a ~122 GiB block
    cache. On the 1 GB Fly VM that is an OOM waiting for the first bulk ingest.
  - Cap both in `db::open` with `surrealdb::opt::Config::datastore_option`
    (`surrealkv_max_memtable_size`, `surrealkv_block_cache_capacity`).
  - Pick the values by measuring peak RSS during a real backfill, not by guessing.
- **Sizing (PRD-0002):** a 1 GB `shared-cpu-1x` VM and a 5 GB volume. Raw pages are about 1 GB
  per NRC series before compression.
- **Outward steps, each needing Aaron's go-ahead:**
  - `gh repo create`, approved 2026-11-12. The auto-mode classifier refuses it even after approval,
    so Aaron runs it himself.
  - `fly volumes create`
  - DNS
  - `CODECOV_TOKEN`, once there is real code to cover

## Analysis notes, for later

- **Heat.**
  - The right predictor depends on cooling type: wet-bulb for tower plants, intake water temperature for
    once-through plants. Coffel & Mankin used dry-bulb air temperature everywhere.
  - A segmented regression per unit separates the *slope* (condenser backpressure, a smooth efficiency
    loss) from the *cliff* (NPDES discharge limits, ultimate-heat-sink tech-spec limits).
  - If the Power column is thermal (see "Still open"), the slope is invisible in the NRC data: the
    reactor stays at 100% while MWe drops. Only the cliff shows up there, so the slope has to come
    from generation data (EIA-923 monthly, EIA-930 hourly). Coffel & Mankin's EIA series has the same
    blind spot.
  - The early-morning snapshot (4–8am ET) undercounts afternoon derates. EIA-923 monthly energy, and
    possibly EIA-930 hourly nuclear output by balancing area, can measure that bias.
- **Coastdowns.** End-of-cycle reactivity declines before a refueling outage. Label them, then exclude
  them from heat.
- **Grid.** The reason text flags grid emergencies ("Maximum Generation Alert"). Measure how much
  capacity is lost *during* those alerts, which bears on how capacity markets credit nuclear.
- **FFD.**
  - Only the 10 CFR 26.719 events that must be reported within 24 hours reach the Event Notifications,
    so this is a subset of positive tests.
  - The 2008 Part 26 rewrite is a break in the series.
  - Pending marijuana rescheduling to Schedule III would change the regime again.
  - Keep a `BREAKS.md`: every definitional or reporting discontinuity, with its citation, before any
    trend line goes up.
