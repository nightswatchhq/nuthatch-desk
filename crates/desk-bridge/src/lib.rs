//! The QObjects of nuthatch-desk, implemented in Rust.
//!
//! Five types, each created and owned by QML:
//!
//! | QObject | Base | Shows |
//! | --- | --- | --- |
//! | [`nest_status`] | `QObject` | one nest's health and heights |
//! | [`table_list`] | `QAbstractListModel` | its catalogue, and which table is previewed |
//! | [`metric_series`] | `QAbstractListModel` | one metric's history |
//! | [`sql_query`] | `QObject` | one statement being run |
//! | [`result_model`] | `QAbstractTableModel` | a result, sortable |
//!
//! The rules they all keep are in `docs/rfc-0001.md`, "Ownership and safety rules". In short:
//! QML owns the object and the Rust struct inside it; only the GUI thread touches either; a worker
//! thread holds a queue handle and owned data, never a pointer; and nothing here panics across
//! the boundary. The logic behind each object lives in `desk-core`, where it is tested without Qt.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod guard;
pub mod metric_series;
pub mod nest_status;
#[cfg(feature = "lifetime-probe")]
pub mod probe;
pub mod result_model;
pub mod sql_query;
pub mod table_list;
