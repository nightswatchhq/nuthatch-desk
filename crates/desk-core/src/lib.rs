//! The state behind each nuthatch-desk QObject, as plain Rust.
//!
//! Nothing here links Qt. The bridge crate holds one of these structs inside each QObject and does
//! no more than copy their output into properties and model signals, so every transition a QObject
//! can make is tested here, on any machine, without a display.

#![deny(missing_docs)]

pub mod catalogue;
pub mod feed;
pub mod format;
pub mod grid;
pub mod results;
pub mod series;
pub mod startup;
pub mod status;
