//! Fixtures shared by unit and integration tests. Always compiled (not `#[cfg(test)]`), so
//! `tests/` can use it too.

use std::future::Future;

use leptos::prelude::LeptosOptions;

/// Options for an in-process router: no site on disk, which only the static-file fallback needs.
#[must_use]
pub fn options() -> LeptosOptions {
    LeptosOptions::builder().output_name("downpower").build()
}

/// Runs `future` on a worker of [`crate::runtime`], whose stacks are deep enough for `SurrealDB` in a
/// debug build; `#[tokio::test]` runs on the 2 MiB test thread.
///
/// # Panics
///
/// When the runtime can't start or `future` panics.
#[allow(clippy::expect_used, reason = "a test fixture")]
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let runtime = crate::runtime().expect("a runtime");
    runtime.block_on(runtime.spawn(future)).expect("the test body panicked")
}
