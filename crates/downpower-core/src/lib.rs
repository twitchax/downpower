//! What the server and the browser share: the eight cause families, the barcode buffer the server
//! packs and the browser draws, the monthly series under the stacked area, and the scram check's
//! counts (DESIGN.md Decisions 17, 23, 24 and 33). Pure, and built for wasm.

pub mod barcode;
pub mod explorer;
pub mod family;
pub mod markers;
pub mod scrams;
pub mod series;

pub use family::{Families, Family};

/// The house result alias.
pub type Res<T> = anyhow::Result<T>;
