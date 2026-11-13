//! Everything between a source and the database: the polite NRC fetcher and the raw store that keeps
//! every fetched page as the record (DESIGN.md Decisions 10 and 15). There is no database here, since
//! the server owns that, so these tests stay fast.

pub mod fetch;
pub mod nrc;
pub mod raw;

/// The house result alias.
pub type Res<T> = anyhow::Result<T>;

/// A result with nothing in it.
pub type Void = Res<()>;
