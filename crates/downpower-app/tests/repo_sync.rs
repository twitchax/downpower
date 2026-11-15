//! Single-source guards. The toolchain is pinned in exactly one place, `rust-toolchain.toml`; these
//! tests fail when a workflow or Dockerfile grows its own pin. A "keep in sync" comment is not
//! enough: conclave's and razel's rotted (ironpad's fix, generalized).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A missing file reads as empty: not every variant has a Dockerfile or release workflow.
fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_default()
}

const PINNABLE: [&str; 3] = [".github/workflows/build.yml", ".github/workflows/release.yml", "Dockerfile"];

#[test]
fn toolchain_is_pinned_only_in_rust_toolchain_toml() {
    let toml = read("rust-toolchain.toml");
    let channel = toml
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .expect("rust-toolchain.toml sets a channel")
        .trim_matches('"');

    for file in PINNABLE {
        let text = read(file);
        assert!(!text.contains(channel), "{file} repeats the toolchain pin `{channel}`; read rust-toolchain.toml instead");
        assert!(!text.contains("nightly-20"), "{file} pins a dated nightly; only rust-toolchain.toml may");
    }
}

#[test]
fn dockerfile_cooks_dependencies_on_the_pinned_toolchain() {
    let dockerfile = read("Dockerfile");
    if dockerfile.is_empty() {
        return;
    }
    let copy = dockerfile.find("COPY rust-toolchain.toml").expect("Dockerfile copies rust-toolchain.toml");
    let cook = dockerfile.find("cargo chef cook").expect("Dockerfile cooks dependencies with cargo-chef");
    assert!(copy < cook, "copy rust-toolchain.toml before `cargo chef cook`, or the dependency layer builds on the wrong toolchain");
}

/// A key of `[[workspace.metadata.leptos]]` in the workspace manifest.
fn leptos(key: &str) -> String {
    let manifest = read("Cargo.toml");
    let section = manifest.split("[[workspace.metadata.leptos]]").nth(1).expect("Cargo.toml has a leptos section");
    section
        .lines()
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| line.strip_prefix(&format!("{key} = ")))
        .unwrap_or_else(|| panic!("the leptos section sets no {key}"))
        .trim_matches('"')
        .to_owned()
}

/// The image's dependency layer is only reused when the cook builds what `cargo leptos build
/// --release` builds: the same packages, profiles, target dirs and compile-time env. Otherwise the
/// final build compiles the dependencies again, as it did for 60 crates and the whole wasm graph.
#[test]
fn dockerfile_cooks_what_cargo_leptos_builds() {
    let dockerfile = read("Dockerfile");
    if dockerfile.is_empty() {
        return;
    }
    let server = format!("cargo chef cook --release --recipe-path recipe.json --package {} --bin {}", leptos("bin-package"), leptos("bin-target"));
    let front = format!(
        "cargo chef cook --profile {} --target wasm32-unknown-unknown --target-dir /app/target/front --recipe-path recipe.json --package {}",
        leptos("lib-profile-release"),
        leptos("lib-package")
    );
    let output = format!("LEPTOS_OUTPUT_NAME={}", leptos("name"));
    let at = |needle: &str| dockerfile.find(needle).unwrap_or_else(|| panic!("the Dockerfile has no `{needle}`"));
    assert!(at(&output) < at(&server).min(at(&front)), "set {output} before cooking: leptos reads it at compile time");
}

/// cargo-leptos fetches the wasm-bindgen CLI that matches Cargo.lock, per project. An installed CLI
/// (in CI, the image, or a task that lands it in ~/.cargo/bin) shadows that download for every
/// project on the machine, at whatever version it happens to be.
#[test]
fn nothing_installs_the_wasm_bindgen_cli() {
    for file in ["Makefile.toml", ".github/workflows/build.yml", ".github/workflows/release.yml", "Dockerfile"] {
        for line in read(file).lines() {
            let code = line.trim_start();
            assert!(
                code.starts_with('#') || !(code.contains("install") && code.contains("wasm-bindgen-cli")),
                "{file} installs wasm-bindgen-cli; let cargo-leptos fetch the version Cargo.lock needs:\n{line}"
            );
        }
    }
}
