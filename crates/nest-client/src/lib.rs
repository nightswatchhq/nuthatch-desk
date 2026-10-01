//! Read-only HTTP client for a running Nuthatch nest.
//!
//! This crate speaks to the nest's HTTP API and nothing else: no store access, no RPC key, no
//! writes. It does not link Qt, so its tests run anywhere, and the terminal client could share it.
//!
//! A nest is not trusted. Every response is read under a size cap ([`Limits`]), every identifier a
//! nest supplies is checked before it is put into SQL ([`sql::is_identifier`]), and numbers keep
//! their digits as text so nothing is rounded on the way through.

#![deny(missing_docs)]

pub mod config;
pub mod endpoints;
mod error;
mod http;
pub mod metrics;
pub mod poll;
pub mod sql;
pub mod tunnel;
pub mod types;

pub use error::Error;
pub use http::{Client, Limits};
