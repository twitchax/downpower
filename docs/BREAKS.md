# Breaks in the series

Every definitional or reporting discontinuity behind the `/ffd` and `/scrams` views, with its
citation, kept before any trend line goes up (DESIGN.md, Analysis notes; PRD-0005 T-003, PRD-0007
T-004). Add an entry before a chart crosses a new one.

## Fitness for duty

Counts are from `catalog/ffd.tsv` (2026-11-18, the home-box archive through 2026-11-13).

### What the series is, and isn't

- **A subset defined by rule.** 10 CFR 26.719(b) makes a licensee report within 24 hours the use,
  sale, distribution, possession or presence of drugs or alcohol in a protected area; violations of
  the FFD policy by licensed operators, supervisors and FFD program personnel; and failures of the
  testing program (blind performance test errors, a laboratory's false results). Rank-and-file
  positive tests reach the NRC only in the annual program reports of 26.717, published as totals.
  So the series counts the reportable events, never the positive tests.
  [10 CFR 26.719](https://www.ecfr.gov/current/title-10/part-26/section-26.719),
  [10 CFR 26.717](https://www.ecfr.gov/current/title-10/part-26/section-26.717).
- **Identified by section.** An event counts when it carries 26.73, 26.719 or 26.417(b)(1), or
  `catalog/ffd.toml` lists it; titles vary and some events have none (DESIGN.md Decision 36).
  The include list holds one event, EN 35619 (1999), filed as a voluntary report with no section.
- **The plant's state.** An event's state is the plant's. The worker's residence is unknown.

### Breaks

#### 2008: Part 26 rewritten

- **What changed.** The Fitness for Duty Programs final rule
  ([73 FR 16966](https://www.govinfo.gov/content/pkg/FR-2008-03-31/html/E8-4998.htm), 2008-03-31)
  replaced Part 26. It took effect 2008-04-30, and licensees could defer parts of it to
  2009-03-31. Reporting moved from 26.73 to 26.719, with a new list of what is reportable.
- **In the data.** The last event coded 26.73 is 2008-04-10 and the first coded 26.719 is
  2008-06-13; none carries both. FFD events averaged 27.6 a year from 1999 to 2007 and 35.8 from
  2009 to 2025.
- **How the views show it.** A dashed rule at 2008-04-30 on every `/ffd` chart. Counts either side
  of it don't compare directly.

#### 2009 to 2023: construction sites

- **What changed.** Subpart K (26.4(f), 26.401 to 26.417) brought FFD programs to construction
  sites, which report under 26.417(b)(1).
  [10 CFR 26.417](https://www.ecfr.gov/current/title-10/part-26/section-26.417).
- **In the data.** 55 events name a Vogtle 3 or 4 docket, 19 of them in 2018, and 11 name Summer
  2 and 3 (2012-06-07 to 2017-04-06). Four are coded 26.417(b)(1) (2013 to 2022).
- **How the views show it.** An event whose units were all before commercial operation (Vogtle 3
  until 2023-07-31, Vogtle 4 until 2024-04-29), or that names only Part 52 dockets the unit catalog
  has no unit for (Summer 2 and 3 never operated), counts apart as a construction-site event and
  stays out of the operating rate (Decision 38). Two 2010 Vogtle events name no unit, so they count
  under other reactor sites.

#### From about 2021: reports stop naming the substance

- **What changed.** Recent reports more often say a person "failed a test specified by the
  fitness-for-duty testing program" without naming what the test found. The wording first appears
  in 2018: once that year, 3 times in 2021, 5 in 2023, 7 in 2025 and 15 in 2026.
- **In the data.** Of the events at operating reactors, the share naming no substance was 13% to
  25% a year from 1999 to 2011 and 42% to 78% from 2021 to 2026 (74% in 2024).
- **How the views show it.** The drug-related and alcohol series are floors, and they sink for a
  reason that isn't behavior, in the same years the adult-use share climbs. So drug-related events
  per 100 unit-years after 2020 don't compare with earlier years; the all-FFD rate isn't affected.
  The count chart's gray segment is the events naming neither, and the page's caveats say so.

#### 2022: the drug testing panel widens

- **What changed.** The Fitness for Duty Drug Testing Requirements final rule
  ([87 FR 71422](https://www.federalregister.gov/documents/2022/11/22/2022-24903/fitness-for-duty-drug-testing-requirements),
  2022-11-22) aligned Part 26 with the HHS Mandatory Guidelines: testing for hydrocodone,
  hydromorphone, oxycodone and oxymorphone, MDMA and MDA, and required special analyses of dilute
  specimens. It took effect 2022-12-22, with compliance required by 2023-11-22.
- **In the data.** A test that finds more substances can find more positives, and the subversion
  checks can find more subversion. The tags would show it as more opiates and more subversion from
  2023 on.
- **How the views show it.** Not marked on the charts yet: the series names opiates twice in 27
  years, too few for a line to cross.

#### 2026: state-licensed medical marijuana moves to Schedule III

- **What changed.** A DEA final rule effective 2026-04-28
  ([91 FR, 2026-04-28, document 2026-08176](https://www.govinfo.gov/content/pkg/FR-2026-04-28/html/2026-08176.htm))
  moved FDA-approved marijuana products and marijuana dispensed under a state medical license to
  Schedule III. Adult-use and other marijuana stay in Schedule I, and a hearing on rescheduling
  the rest was under way in 2026.
- **In the data.** Nothing yet. Part 26 treats Schedule I substances as illegal drugs and others by
  prescription, so a licensee's handling of a state medical card may change. Whether the NRC changes
  Part 26 or its guidance is open; watch the 26.719 events from mid-2026.
- **How the views show it.** Not marked. The 2026 column is partial in any case.

#### Legalization dates

- **What changed.** `catalog/cannabis.csv` counts a medical program only when NCSL calls it
  comprehensive. NCSL now counts Texas (HB 46, effective 2025-09-01) and Georgia (SB 220,
  effective 2026-07-01), whose earlier low-THC laws didn't qualify, so both states' operating
  unit-years enter the medical share in those years.
- **How the views show it.** The medical share steps up in 2025 and 2026 by those states' share of
  the fleet. A correction to a date is one edit in the CSV, and each row cites its source.

## Scrams

Counts are from `catalog/triggers.tsv` and the raw store (2026-11-23, the home-box archive through
2026-11-19).

### What the series is, and isn't

- **Scrams the NRC was notified of.** A scram counts when an unretracted Event Notification's
  unit table gives the unit a SCRAM Code other than `N` (DESIGN.md Decisions 30 and 33). Under
  10 CFR 50.72 an actuation of the reactor protection system while critical is a 4-hour report,
  (b)(2)(iv)(B), and a valid actuation while shut down an 8-hour one, (b)(3)(iv)(A).
  [10 CFR 50.72](https://www.ecfr.gov/current/title-10/chapter-I/part-50/section-50.72).
- **The system is what the report says started the scram,** as the phrase rules in
  `catalog/triggers.toml` read it: the most upstream cause the text names, or the signal's system
  when it names only the trip signal (Decision 41). 40% of the reports leave the root cause under
  investigation, and the Licensee Event Reports that name it later aren't a source here. On 50
  scrams the rules weren't tuned on, they agreed with a hand label on 39 systems and 43
  circumstances (PRD-0007.log.md#t-002).
- **An episode runs from the drop to the return** within 10 points of the pre-scram power, its
  energy measured below that power and counted in the scram's year (Decision 42).

### Breaks

#### 2001: 10 CFR 50.72 rewritten

- **What changed.** The Reporting Requirements for Nuclear Power Reactors final rule
  ([65 FR 63769](https://www.federalregister.gov/documents/2000/10/25/00-27283/reporting-requirements-for-nuclear-power-reactors-and-independent-spent-fuel-storage-installations),
  2000-10-25) revised 50.72 and 50.73, effective 2001-01-23. Scrams moved from 50.72(b)(2)(ii),
  coded "ESF ACTUATION" or "RPS ACTUATION", to (b)(2)(iv)(B), "RPS ACTUATION - CRITICAL", and
  (b)(3)(iv)(A), "VALID SPECIF SYS ACTUATION".
- **In the data.** The first scram coded (b)(2)(iv)(B) is EN 37691 on 2001-01-25, and the last
  coded (b)(2)(ii) is EN 37994 on 2001-05-16. Scram events number 100 in 2000 and 101 in 2001, so
  the count shows no step.
- **How the views show it.** Not marked: a scram is identified by its SCRAM Code and tagged from
  its text, neither of which the rule changed.

#### From about 2021: titles stop naming the trigger

- **What changed.** Recent reports carry a generic title ("AUTOMATIC REACTOR TRIP") and a
  templated text ("The trip was not complex, with all systems responding normally").
- **In the data.** A title naming no trigger is 76 of 661 scram events in 1999–2005 (11%) and 138
  of 226 since 2021 (61%). The tags' "not named" share rises from about 2% of scrams in 2000–2004
  to 5% in 2020–2024 and 10% in 2025–2026.
- **How the views show it.** The rules read every sentence, not the title, so the break moves the
  "not named" share rather than the systems' rates; "not named" is drawn in every chart.

#### From 2020: revised texts

- **What changed.** Some events carry an "EN Revision Imported Date" and an "EN Revision Text",
  the NRC's revision of the original notification.
- **In the data.** 80 scram events, from one in 2020 to four in 2026 (37 in 2021). The tags read
  whichever text the page holds.
- **How the views show it.** Not marked: a revision changes an event's text, not whether it
  counts.

#### SCRAM Codes `A` and `M`

- **What changed.** Nothing datable. Nearly every scrammed unit is coded `A/R` or `M/R`, and the
  NRC doesn't document what separates `A` and `M` from them; several `A` units were shut down.
- **In the data.** 23 units coded `A` and 5 coded `M` from 1999 to 2024, against 1,794 coded
  `A/R` or `M/R`.
- **How the views show it.** They count as automatic and manual scrams. One from under 10% power,
  as a shut-down unit's is, has no episode.
