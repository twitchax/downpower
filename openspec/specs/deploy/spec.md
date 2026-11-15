# deploy Specification

## Purpose
How downpower ships and runs in production: the container image and its entrypoint, the published image, the Fly app `fly.toml` describes, the smoke test that checks the image, and how a raw store shipped onto the volume becomes the served ledger.

## Requirements
### Requirement: The image serves by default
The image SHALL run `/app/downpower` with no subcommand, so it serves, from `/app` with the hashed site under `/app/site`, listening on `0.0.0.0:3000`, with `DOWNPOWER_DATA_DIR=/data` and the settings cargo-leptos would pass (`LEPTOS_OUTPUT_NAME=downpower`, `LEPTOS_SITE_ROOT=/app/site`, `LEPTOS_SITE_PKG_DIR=pkg`, `LEPTOS_HASH_FILES=true`, `LEPTOS_ENV=PROD`). (PRD-0001 T-001, PRD-0002 T-016)

#### Scenario: Starting the image
- **WHEN** the image starts with a volume at `/data` and its port 3000 published
- **THEN** `/healthz` answers `ok` on that port

### Requirement: tini as init
The image SHALL start the server under `tini -s`, so zombies are reaped and `docker stop`'s SIGTERM reaches the server, whether tini runs as PID 1 or under Fly's own init. (PRD-0001 T-001, PRD-0002 T-016)

#### Scenario: Stopping the container
- **WHEN** `docker stop` signals the container
- **THEN** the server receives the SIGTERM and exits

### Requirement: The entrypoint hands the volume to the server
Started as root, the entrypoint SHALL create `$DOWNPOWER_DATA_DIR` (default `/data`), chown to 10001:10001 whatever under it uid 10001 doesn't own, and then run the server as uid and gid 10001 with no supplementary groups. Started as any other user, it SHALL run the server unchanged. (PRD-0002 T-016)

#### Scenario: A raw store put over fly ssh
- **WHEN** the volume holds a root-owned `/data/raw/shipped` and the image starts
- **THEN** nothing under `/data` is left unowned by uid 10001
- **AND** the server process runs as uid 10001

### Requirement: Smoke test
`docker/smoke.sh <image>`, run in CI and as `cargo make docker-smoke`, SHALL start the image over a volume holding a root-owned file and fail unless `/healthz` answers within about 30 seconds, nothing under `/data` is left unowned by uid 10001, and the server runs as uid 10001. (PRD-0002 T-016)

#### Scenario: A healthy image
- **WHEN** the smoke test runs against an image built from the tree
- **THEN** it prints `smoke: ok` and exits 0

### Requirement: Published image
CI SHALL build the image on every run and smoke-test it before any push, and SHALL push it to `ghcr.io/twitchax/downpower` as `latest` and `sha-<commit>` from `main` only. (PRD-0001 T-001, PRD-0002 T-016)

#### Scenario: A push to main
- **WHEN** a commit lands on `main` and its image passes the smoke test
- **THEN** `ghcr.io/twitchax/downpower:latest` and `:sha-<commit>` are that image

### Requirement: The Fly app
`fly.toml` SHALL run the app `twitchax-downpower` in `sjc` on a `shared-cpu-1x` machine with 1 GB, always on (auto-stop off, one machine at least), with the 5 GB volume `downpower_data` at `/data`, `DOWNPOWER_FETCH=daily` and `RUST_LOG=info`, forced HTTPS in front of internal port 3000, and a `GET /healthz` check every 30 seconds after a 30-second grace period. (PRD-0001 T-001, PRD-0002 T-011 T-016)

#### Scenario: A deploy
- **WHEN** `fly deploy` ships a new image
- **THEN** the machine mounts `/data`, runs the update loop with fetching on, and passes the `/healthz` check

### Requirement: The live site
The deployed app SHALL serve `https://downpower.twitchax.com` under Fly's certificate, with its health check answering `ok` there. (PRD-0002 T-016)

#### Scenario: The health check from outside
- **WHEN** a client runs `curl -fsS https://downpower.twitchax.com/healthz`
- **THEN** it prints `ok`

### Requirement: A shipped raw store builds on the next start
A raw store copied onto the volume SHALL become the served ledger on the next `serve` start without any other step, since its digest differs from the current build's (as `ledger` describes), while any earlier build keeps serving until the new one lands. (PRD-0002 T-010 T-016)

#### Scenario: The first ship
- **WHEN** a deploy with fetching off is followed by unpacking `raw.tar` into `/data` and another deploy
- **THEN** the second boot rebuilds the ledger from the shipped pages in the background
