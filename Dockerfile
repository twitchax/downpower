# syntax=docker/dockerfile:1
#
# Multi-stage build. cargo-chef caches the server's dependency layer; cargo-leptos then builds the
# server and the hashed wasm/js/css site in one step.

FROM rust:1-bookworm AS chef
# Prebuilt tools through cargo-binstall: building them from source costs minutes per cold build.
# The newest cargo-leptos; it fetches the wasm-bindgen CLI matching Cargo.lock itself.
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall --no-confirm cargo-chef cargo-leptos
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# The pinned toolchain (with its wasm target) goes in BEFORE cooking, so dependencies and the final
# build share it and the cooked layer is reused.
COPY rust-toolchain.toml .
RUN rustup toolchain install
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo leptos build --release

FROM debian:stable-slim AS runtime
# tini runs as PID 1: it reaps zombies and forwards `docker stop`'s SIGTERM to the server.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /nonexistent --shell /usr/sbin/nologin app
WORKDIR /app
COPY --from=builder /app/target/release/downpower /app/downpower
COPY --from=builder /app/target/site /app/site
# hash-files: leptos reads hash.txt next to the binary to find the hashed assets.
COPY --from=builder /app/target/release/hash.txt /app/hash.txt
USER app
# The [workspace.metadata.leptos] settings, as the env vars cargo-leptos would have passed.
ENV LEPTOS_OUTPUT_NAME=downpower \
    LEPTOS_SITE_ROOT=/app/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:3000 \
    LEPTOS_HASH_FILES=true \
    LEPTOS_ENV=PROD
EXPOSE 3000
ENTRYPOINT ["/usr/bin/tini", "--", "/app/downpower"]
