//! Fixtures shared by unit and integration tests. Always compiled (not `#[cfg(test)]`), so
//! `tests/` can use it too.

use leptos::prelude::LeptosOptions;

/// Options for an in-process router: no site on disk, which only the static-file fallback needs.
#[must_use]
pub fn options() -> LeptosOptions {
    LeptosOptions::builder().output_name("downpower").build()
}
