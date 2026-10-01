//! The documents a nest serves, as far as this client reads them.

use serde::{Deserialize, Deserializer};

/// A field the nest may omit or send as `null`, read as its default either way.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// `GET /ready`. A stalled or quarantined nest serves the same document under a 503.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Ready {
    /// The nest's own verdict.
    #[serde(default, deserialize_with = "nullable")]
    pub ready: bool,
    /// No poll has succeeded for longer than the nest tolerates.
    #[serde(default, deserialize_with = "nullable")]
    pub stalled: bool,
    /// The cursor cannot advance.
    #[serde(default, deserialize_with = "nullable")]
    pub wedged: bool,
    /// The first poll after start failed.
    #[serde(default, deserialize_with = "nullable")]
    pub initial_poll_failed: bool,
    /// A direct-to-sealed backfill pass has stopped making progress.
    #[serde(default, deserialize_with = "nullable")]
    pub seal_direct_stalled: bool,
    /// An authored entity has stopped folding.
    #[serde(default, deserialize_with = "nullable")]
    pub entities_stalled: bool,
    /// The runtime has set this nest aside after a fault.
    #[serde(default, deserialize_with = "nullable")]
    pub quarantined: bool,
    /// Chain tip. Null for a cursorless role, which has no tip to lag behind.
    pub tip: Option<u64>,
    /// Blocks between the tip and the last indexed block. Null for a cursorless role.
    pub lag_blocks: Option<u64>,
    /// Last block indexed into the hot store.
    #[serde(default, deserialize_with = "nullable")]
    pub last_block: u64,
    /// Last block sealed to Parquet.
    #[serde(default, deserialize_with = "nullable")]
    pub sealed_through: u64,
    /// Seconds since the nest last polled its RPC. Null before the first poll.
    pub seconds_since_poll: Option<u64>,
    /// How the nest paces itself.
    pub freshness: Option<Freshness>,
    /// A direct-to-sealed backfill pass is running.
    #[serde(default, deserialize_with = "nullable")]
    pub seal_direct_active: bool,
    /// First block of the backfill pass.
    pub seal_direct_origin: Option<u64>,
    /// Last block the backfill pass has completed.
    pub seal_direct_completed: Option<u64>,
    /// Block the backfill pass is heading for.
    pub seal_direct_target: Option<u64>,
    /// The nest's version. Published from Nuthatch 3.9.0.
    pub version: Option<String>,
}

/// The `freshness` object of [`Ready`].
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Freshness {
    /// How often the nest polls its RPC.
    pub poll_interval_secs: Option<u64>,
}

/// `GET /tables`: the nest's catalogue.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Tables {
    /// The tables, in the nest's order.
    #[serde(default)]
    pub tables: Vec<EventTable>,
}

/// One table of the catalogue.
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct EventTable {
    /// The name to use in SQL.
    pub table: String,
    /// The columns, implicit ones included.
    #[serde(default)]
    pub columns: Vec<Column>,
    /// The contract alias the table belongs to.
    #[serde(default)]
    pub alias: String,
    /// `call` for a table of `eth_call` results rather than decoded events.
    #[serde(default)]
    pub kind: String,
}

impl EventTable {
    /// Whether the table holds `eth_call` results.
    pub fn is_call(&self) -> bool {
        self.kind == "call"
    }

    /// The heading the table is listed under. State calls each have an alias of their own, so they
    /// go together under one heading rather than a heading apiece.
    pub fn group(&self) -> &str {
        if self.is_call() {
            "calls"
        } else if !self.alias.is_empty() {
            &self.alias
        } else {
            self.table.split_once("__").map_or("", |(alias, _)| alias)
        }
    }

    /// The name as listed under its heading, which already says the alias.
    pub fn short_name(&self) -> &str {
        self.table
            .strip_prefix(self.group())
            .and_then(|rest| rest.strip_prefix("__"))
            .unwrap_or(&self.table)
    }
}

/// One column of an [`EventTable`].
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct Column {
    /// The name to use in SQL.
    pub name: String,
    /// The Solidity type, or `implicit` for a column every row carries.
    #[serde(default)]
    pub sol_type: String,
}

/// `GET /nest`: what the nest is.
#[derive(Debug, Deserialize, Default)]
pub(crate) struct NestDocument {
    pub(crate) name: Option<String>,
    pub(crate) chain: Option<String>,
}

/// `GET /`: the root document.
#[derive(Debug, Deserialize, Default)]
pub(crate) struct RootDocument {
    /// Rows in the nest's hot store, not sealed history.
    pub(crate) entities: Option<u64>,
}

/// `GET /queries`: what the SQL surface accepts.
#[derive(Debug, Deserialize)]
pub(crate) struct QueriesDocument {
    #[serde(default)]
    pub(crate) sql: String,
    #[serde(default = "yes")]
    pub(crate) free_form: bool,
    #[serde(default)]
    pub(crate) queries: Vec<NamedQuery>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NamedQuery {
    pub(crate) name: String,
}

fn yes() -> bool {
    true
}

/// Whether the nest takes free-form SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlAccess {
    /// Any `SELECT` is answered.
    Open,
    /// Only the operator's declared queries are answered.
    Closed {
        /// The mode as the nest names it.
        mode: String,
        /// The declared queries.
        named: Vec<String>,
    },
}

/// What the nest is, as opposed to how it is doing. Nuthatch builds all of it at startup and never
/// changes it, so it is fetched once and again only after a restart.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    /// The authored nest name.
    pub nest_name: Option<String>,
    /// The chain the nest indexes.
    pub chain: Option<String>,
    /// The catalogue.
    pub tables: Tables,
    /// Whether free-form SQL is open.
    pub sql: SqlAccess,
}

/// `GET /nests` on a runtime's root: the nests it mounts.
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct Roster {
    /// The nests, each serving its own API under [`RosterNest::path`].
    pub nests: Vec<RosterNest>,
}

/// One nest of a [`Roster`].
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RosterNest {
    /// The mount's name.
    pub name: String,
    /// Where the nest serves its API, when that is not `/{name}`.
    #[serde(default)]
    pub base_path: String,
}

impl RosterNest {
    /// The path under the runtime's root where this nest serves its API.
    pub fn path(&self) -> String {
        if self.base_path.is_empty() {
            format!("/{}", self.name)
        } else {
            self.base_path.clone()
        }
    }
}
