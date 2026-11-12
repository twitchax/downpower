# downpower: project instructions

Why US nuclear reactors lose output: a public-data dashboard of derates, outages, and NRC events by cause

The global conventions in `~/CLAUDE.md` and the `bootstrap-rust-project` skill's `conventions.md`
apply in full. This file records only what is specific to downpower; **where it is silent,
defer to `~/CLAUDE.md`.**

## Architecture

A Leptos app (SSR + hydration on axum, built by cargo-leptos), split three ways:
`crates/downpower-app` (components, pages and the shell; `ssr` / `hydrate` features),
`crates/downpower-frontend` (the wasm entry point), and `crates/downpower-server` (the
axum binary). Styling is the `instrument-design-language` tokens in `style/_tokens.scss`, with
Lucide icons from `icondata_lu`, **without the background grid**. **No component library.**

The store is embedded SurrealDB 4 (`kv-surrealkv`) behind `crates/downpower-server/src/db.rs`,
under `DOWNPOWER_DATA_DIR` (default `./data`; `/data`, the Fly volume, in production). Server-only:
the wasm half never links it. It deploys to Fly (`fly.toml`); the decisions so far and the open
design questions are in **`docs/DESIGN.md`**. Read it before starting any feature work.

## Commands

```bash
cargo make ci        # fmt-check + clippy (-D warnings) + nextest: the gate, run verbatim in CI
cargo make test      # nextest
cargo make uat       # ci + the #[ignore]d slow suites: what PRD UATs point at
cargo make codecov   # coverage.lcov
```

## Conventions

- **Toolchain:** pinned in `rust-toolchain.toml`. It is **stable, coverage included**, against the
  house nightly default, because SurrealDB 4 doesn't build on current nightlies (the reason and
  the probe for switching back are in that file). Nothing else may pin a toolchain; `repo_sync`
  tests enforce it.
- **Lints:** the `[workspace.lints]` table in `Cargo.toml` (`all` + `pedantic` + `unused` +
  `unwrap_used` / `expect_used` denied). No new `allow`s beyond a narrow, commented one, and never
  for `too_many_*` or `needless_pass_by_value`: refactor instead.
- **Errors:** `anyhow` through the `Res` / `Void` aliases, with `.context()` on every `?` that crosses an I/O or parse boundary.
- **Tests:** unit tests in-module, integration and e2e in `tests/` (one subsystem per file, `e2e_*`
  names for tests that spawn the binary), fixtures from the `tests` factory module. Every
  behavioral change ships a test.
- **PRDs** live in `.prds/`; reference task IDs in commits, and never mark a UAT verified without a
  real, passing test.
- **No AI attribution** anywhere: no `Co-Authored-By` trailers, session links or "generated with"
  footers in commits, PRs, changelogs or docs.
