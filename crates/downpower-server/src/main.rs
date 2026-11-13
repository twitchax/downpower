//! The `downpower` binary: set up tracing, then serve the site or fetch NRC pages (`cli`).

// `cargo llvm-cov` on nightly sets `coverage_nightly`; elsewhere this is inert, so the source builds
// on stable. Enable the feature only in a crate root that uses `#[coverage(off)]`: an unused
// feature fails the `unused` lint group on current nightlies.
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::io::IsTerminal as _;

use anyhow::Context as _;
use clap::Parser as _;
use leptos::prelude::get_configuration;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use downpower_server::cli::{self, Cli, Command};

#[cfg_attr(coverage_nightly, coverage(off))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let data_dir = downpower_server::db::data_dir(std::env::var_os(downpower_server::db::DATA_DIR_VAR));
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve(&data_dir).await,
        Command::Fetch(args) => {
            let summary = cli::fetch(&args, &data_dir).await?;
            tracing::info!(summary.stored, summary.missing, summary.skipped, "fetch done");
            Ok(())
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn serve(data_dir: &std::path::Path) -> anyhow::Result<()> {
    // cargo-leptos passes [workspace.metadata.leptos] as LEPTOS_* env vars; the Dockerfile sets
    // them for the container.
    let options = get_configuration(None).context("reading the Leptos configuration")?.leptos_options;

    // Open the store before binding, so a missing or unwritable volume fails the deploy's health
    // check rather than the first ingest. Held for the server's lifetime; the ingest task will
    // take a clone.
    let _store = downpower_server::db::open(data_dir).await?;
    tracing::info!(data_dir = %data_dir.display(), "store open");

    let addr = options.site_addr;
    let listener = TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, downpower_server::router(options)).await.context("serving")
}
