//! Compiles `findings/*.md` into the app (DESIGN.md Decision 44, PRD-0008 T-001). Each file is
//! parsed and its note rendered by `downpower_core::findings::render`, a bad one failing the build,
//! and the table is written to `$OUT_DIR/findings.rs`. `findings/README.md` describes the format
//! and isn't a finding. Without the directory (cargo-chef's dependency cook has none) the table is
//! empty.

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::Context as _;
use downpower_core::findings::render;

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../findings");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    if dir.exists() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|name| name.to_str()).with_context(|| format!("{} isn't UTF-8", path.display()))?;
            if name == "README.md" {
                continue;
            }
            let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            files.push((name.to_owned(), text));
        }
    }
    let findings = render::parse_all(files)?;
    let out = PathBuf::from(env::var("OUT_DIR")?).join("findings.rs");
    fs::write(&out, render::table(&findings)).with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}
