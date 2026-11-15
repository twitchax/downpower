# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each release opens with a sentence on
why the version moved the way it did.

## [Unreleased]

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
