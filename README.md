# downpower

[![Build and Test](https://github.com/twitchax/downpower/actions/workflows/build.yml/badge.svg)](https://github.com/twitchax/downpower/actions/workflows/build.yml)
[![codecov](https://codecov.io/gh/twitchax/downpower/branch/main/graph/badge.svg)](https://codecov.io/gh/twitchax/downpower)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Why US nuclear reactors lose output: a public-data dashboard of derates, outages, and NRC events by cause

## Usage

```text
$ downpower --help
TODO: paste the real `--help` output once the CLI settles.
```

`cargo make dev` serves the app with live reload; `cargo make build-site` builds the release
server and hashed assets under `target/site`.

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

## License

MIT
