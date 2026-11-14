# downpower

[![Build and Test](https://github.com/twitchax/downpower/actions/workflows/build.yml/badge.svg)](https://github.com/twitchax/downpower/actions/workflows/build.yml)
[![codecov](https://codecov.io/gh/twitchax/downpower/branch/main/graph/badge.svg)](https://codecov.io/gh/twitchax/downpower)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Why US nuclear reactors lose output: a public-data dashboard of derates, outages, and NRC events by cause

Live at [downpower.twitchax.com](https://downpower.twitchax.com); [/status](https://downpower.twitchax.com/status) says what the ledger rests on.

## Usage

```text
$ downpower --help
Why US nuclear reactors lose output

Usage: downpower [COMMAND]

Commands:
  serve     Serve the site (the default)
  fetch     Fetch NRC pages (skipping the pages an earlier run settled) or EIA's nuclear outage file into the raw store
  backfill  Fetch every series through today, then rebuild the ledger, failing on any unit or tag the catalog doesn't know. A first run takes about a day
  rebuild   Rebuild the ledger from the raw store, failing on any unit or tag the catalog doesn't know. The store must be free: stop `serve` first
  help      Print this message or the help of the given subcommand(s)
```

`cargo make dev` serves the app with live reload; `cargo make build-site` builds the release
server and hashed assets under `target/site`.

### Fetching the NRC archive

`downpower fetch --series status` (or `--series events`) reads every page from 1999 on into
`$DOWNPOWER_DATA_DIR/raw` (default `./data/raw`): zstd blobs named by sha256, plus
`manifest.jsonl`. It sends one request every 5 seconds at most, under the User-Agent
`downpower/0.1 (+https://github.com/twitchax/downpower)`, and stops on a 403. Rerunning it skips
every page that is already settled, so an interrupted run picks up where it stopped. A full status
pass takes about 14 hours, and the events pass about 10.

`downpower fetch --series nuc-status` stores EIA's keyless `NUC_STATUS` bulk file (daily power and
capacity per generator from 2007) in the same raw store, in one request.

`downpower serve` builds the ledger (one row per in-service unit-day) from the raw store into
SurrealDB when the stored build is missing, was made by another parser or catalog, or was built
from other raw content. With `DOWNPOWER_FETCH=daily` (as on Fly), it also fetches every six hours:
the newest status page, the one that has just passed its 28-day hold, recent Event Notification
pages, and a weekly `NUC_STATUS`. It rebuilds in the background, about 30 seconds for the
whole archive, and keeps serving the previous build until the new one is done. Unknown tags and
unit names are quarantined there; `downpower rebuild` and `downpower backfill` fail on them instead.

### Shipping the raw store to Fly

The backfill runs on the home box, and the Fly volume gets only the raw store: the deployed server
builds its own ledger from it.

```bash
downpower backfill                       # fetch everything, then rebuild and fail on anything unknown
tar -C data -cf raw.tar raw              # the blobs are zstd already
fly volumes create downpower_data -a twitchax-downpower -r sjc -s 5
fly deploy --ha=false -e DOWNPOWER_FETCH=off  # the first boot fetches nothing, so the tar lands alone
fly ips allocate-v6 -a twitchax-downpower && fly ips allocate-v4 --shared -a twitchax-downpower
fly ssh sftp put -a twitchax-downpower raw.tar /data/raw.tar
fly ssh console -a twitchax-downpower -C "sh -c 'tar -xf /data/raw.tar -C /data && rm /data/raw.tar'"
fly deploy --ha=false                    # fetching on, as fly.toml sets it
```

A deploy of a prebuilt image (`--image`) doesn't allocate the app's public addresses, hence the
`ips` line; a shared IPv4 and an IPv6 cost nothing.

Every deploy passes `--ha=false`: a second machine would get its own volume and its own update loop.
The image's entrypoint hands whatever `fly ssh` wrote as root to the server's user at boot. On the
second deploy, `serve` sees raw content its last build didn't read and rebuilds in the background.
The tar replaces `/data/raw/manifest.jsonl`, so this is for the first ship: after that, the update
loop keeps the volume's raw store current itself.

## Install

```bash
docker run --rm ghcr.io/twitchax/downpower:latest
```

## Development

```bash
cargo make ci        # fmt-check + clippy (-D warnings) + nextest: the gate CI runs
cargo make uat       # ci + the #[ignore]d slow suites
cargo make codecov   # coverage.lcov
```

The reason classifier's tables live in `catalog/`. After a fetch, `cargo xtask reasons` regenerates
`catalog/reasons.tsv` (every distinct tag on the stored status pages), `cargo xtask rules` shows what
each key and rule in `catalog/tags.toml` catches, and `cargo xtask unclassified` rewrites the list of
tags accepted as Unattributed. CI fails when a tag in the snapshot is unknown. `cargo xtask summary`
builds the ledger as a rebuild does and prints lost output by family, method and cause, and
`cargo xtask eia` compares the stored status pages with the newest stored `NUC_STATUS`.
`cargo xtask capacity <dir>` rewrites `catalog/capacity.csv` (net summer capacity for 1999 to 2006)
from the EIA-860 zips in `<dir>`: `eia860a1999.zip` and `eia860a2000.zip` from
`https://www.eia.gov/electricity/data/eia860/eia860a/`, and `eia8602001.zip` to `eia8602006.zip`
from `https://www.eia.gov/electricity/data/eia860/archive/xls/`.

## License

MIT
