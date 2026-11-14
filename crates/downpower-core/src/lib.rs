//! What the server and the browser share: the eight cause families, the barcode buffer the server
//! packs and the browser draws, and the monthly series under the stacked area (DESIGN.md Decisions
//! 17, 23 and 24). Pure, and built for wasm.

pub mod barcode;
pub mod family;
pub mod series;

pub use family::{Families, Family};

/// The house result alias.
pub type Res<T> = anyhow::Result<T>;
