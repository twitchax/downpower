# downpower: design notes

**Status:** pre-PRD. These are the decisions from the kickoff grilling on 2026-11-11. The open questions
below are the first round of the next session, and the first PRD is cut from the answers
(`prd-management`).

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

## Open questions: the next round

Each question carries the recommendation from the kickoff. None of them is settled. The facts behind
them are in `docs/research/data-sources.md`.

0. **Getting the NRC data legitimately. This blocks everything else.**
   - Live nrc.gov returns an Akamai 403 to every non-browser client, and spoofing a browser is out.
   - The recommendation is a three-layer source plan:
     - **(a)** EIA's `generator-nuclear-outages` API (daily, 2007 to now, built from the NRC report) as
       the numeric backbone.
     - **(b)** Reason text from the NRC daily HTML pages, read through Wayback captures at a polite rate
       for the backfill.
     - **(c)** A request to the NRC (open-gov or webmaster contact) for bulk access or an allowlist,
       to cover the daily update and the 28-day reason fill-in.
   - Before choosing, test whether the 403 also applies from a Fly machine's IP.
   - If none of this works, the reason text (the whole point) is backfill-only, and the daily update is
     numbers-only from EIA.
1. **What SurrealDB is responsible for.** The recommendation:
   - Raw pages are content-addressed files on the volume.
   - Surreal holds the normalized and derived tables.
   - The database is always rebuildable from the raw pages.
   - The alternative is Surreal as the system of record for raw pages too.
2. **Classifying the reason strings.** The vocabulary turned out small: about 54 distinct strings a
   month, built from about 30 atomic `;`-joined tags. The recommendation:
   - Split each string into tags.
   - Map each normalized tag to a cause through a committed, reviewed lookup table.
   - Fail the ingest loudly on any tag the table doesn't know, so the table grows on purpose.
   - Use an LLM only to propose entries for new tags, never at runtime.
   - Open sub-question: how to split "Environmental Conditions" (heat versus kelp versus swell). One
     option is to join it with temperature data.
3. **Charting.** The recommendation:
   - Hand-rolled SVG components. The house precedent is the control-tower ridgelines and the
     productivity-triage small multiples, where the geometry lives in a pure module and the view only
     places it. No Leptos UI of Aaron's uses a chart crate.
   - Canvas only for the dense unit × day barcode: about 95 units × 10K days ≈ 1M cells, which is too
     much for SVG.
   - Load the `dataviz` skill before the first chart.
4. **Hero view and first milestone.** The candidates:
   - the fleet barcode (unit × day, colored by cause)
   - lost MWh by cause as a stacked area over time
   - a per-unit heat-tolerance curve
   - FFD by state, with markers at legalization dates
5. **Joins and their order.** The sources: NOAA (air temperature and wet-bulb), USGS (intake water
   temperature), EIA-860 Schedule 6 (cooling type), EIA-923 Schedule 8D (cooling water temperatures),
   EIA-923 monthly generation, and EIA-930 hourly output.
6. **Domain.** `downpower.twitchax.com`, or something else.

## Deploy

- `fly.toml` is at the root.
  - App `twitchax-downpower`, region `sea`.
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
- **Outward steps not yet taken; each needs Aaron's go-ahead:**
  - `gh repo create`
  - `fly apps create` and `fly volumes create`
  - DNS

## Analysis notes, for later

- **Heat.**
  - The right predictor depends on cooling type: wet-bulb for tower plants, intake water temperature for
    once-through plants. Coffel & Mankin used dry-bulb air temperature everywhere.
  - A segmented regression per unit separates the *slope* (condenser backpressure, a smooth efficiency
    loss) from the *cliff* (NPDES discharge limits, ultimate-heat-sink tech-spec limits).
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
