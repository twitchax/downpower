# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each release opens with a sentence on
why the version moved the way it did.

## [Unreleased]

### Fixed

- The stacked area's table view rides in the URL as `table=1`, so a reload or a shared link keeps
  it open.

## [0.1.0] - 2026-11-18

The first tagged release keeps the scaffold's version, since nothing earlier was ever tagged or
published, and covers everything deployed at downpower.twitchax.com through the events explorer.

### Added

- Initial scaffold.
- The status-report ledger (PRD-0002): every NRC Power Reactor Status Report and Event
  Notification page since 1999 in a content-addressed raw store, fetched politely from nrc.gov;
  EIA's `NUC_STATUS` for capacity and the days the NRC pages miss; one row per in-service unit-day
  with its reason tags classified into causes and eight families, and lost output from net
  capacity.
- `downpower fetch | backfill | rebuild | serve`, and an update loop in `serve` that fetches every
  six hours, updates the ledger in place, and keeps the previous build serving through a rebuild.
- The fleet barcode and the stacked area of lost output by cause on the home page, sharing
  `?from&to&layer&families`, and `/status` with coverage, agreement and quarantine.
- The Fly deploy at downpower.twitchax.com.
- Event Notifications (PRD-0003): every stored page parsed into events with their 10 CFR sections,
  unit tables (SCRAM Code and power) and update timelines, retracted by the page's banner, each
  facility identified by docket, and the event set stored beside every build.
- `See EN#` references on status days resolve to a cause (`scram`, `required_shutdown` or
  `en_reported`) through `catalog/xref.toml`, with the event as evidence on the unit-day.
- The internal scram check in every build: EN scrams against the status reports' drops, and
  status-report scrams against the events, on `/status`, `/api/status` and `cargo xtask scrams`.
- Event Notification pages fetched on weekends too, and held 28 days like the status pages.
- The NRC's scram dataset, fetched weekly into the raw store and reconciled with our EN scrams by
  event number and docket in every build (`downpower fetch --series scram-dataset`,
  `cargo xtask scrams`, `/api/status`).
- `/status` shows the Event Notification archive (pages, events by category, parse failures and
  quarantined facility names) and the scram dataset's agreement beside the internal check.
- `cargo xtask oracle` and a parser test against the NRC's raw data file of 54 events.
- The events explorer (PRD-0004): `/events` filters every Event Notification by date, category,
  state, place, unit, 10 CFR section, emergency class, scram and text, with a counts-per-month
  chart, and `/events/{number}` shows one event in full with its NRC link and each unit's barcode
  strip. `/api/events`, `/api/events/counts`, `/api/events/{number}` and `/api/markers` serve them.
- Event markers on the barcode, off by default (`events=1`), a diamond for a scram; the barcode
  readout names the event a day's cause came from, and a click opens it.

### Changed

- The tag table gives `scram` only to a reactor trip. Scram time tests, tripped pumps, lines and
  breakers, trip-system tests and parts, and turbine or generator trips that tripped no reactor
  take their own causes.

[Unreleased]: https://github.com/twitchax/downpower/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/twitchax/downpower/releases/tag/v0.1.0
