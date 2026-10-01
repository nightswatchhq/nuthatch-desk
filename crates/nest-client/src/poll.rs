//! One poll of one nest, and the little a poller remembers between polls.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    Client, Error,
    endpoints::Problem,
    sql::{Preview, Selection},
    types::{Identity, Ready, Roster, SqlAccess},
};

/// What one poll should fetch beyond the nest's health.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    /// The table to preview, by name, so a catalogue refetched after a restart keeps the place.
    pub table: Option<String>,
    /// How many of its newest rows to fetch.
    pub rows: usize,
}

/// Everything one poll learned. Owned, so it can be handed to another thread whole.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// When the poll finished.
    pub taken: Instant,
    /// How long it took.
    pub elapsed: Duration,
    /// `/ready`.
    pub ready: Result<Ready, Error>,
    /// `/metrics`, one number per family.
    pub metrics: Result<BTreeMap<String, f64>, Error>,
    /// Rows in the hot store.
    pub hot_rows: Result<Option<u64>, Error>,
    /// What the nest is. Kept from an earlier poll once known.
    pub identity: Option<Arc<Identity>>,
    /// Why the identity could not be fetched, when it could not.
    pub identity_problem: Option<Problem>,
    /// Set when the URL is a runtime's root, which mounts nests rather than being one.
    pub roster: Option<Roster>,
    /// The preview asked for, when SQL is open and the table exists.
    pub selection: Option<Result<Selection, Error>>,
    /// A counter went backwards since the last poll: the nest is a new process.
    pub restarted: bool,
}

/// Counters that only rise for the life of a Nuthatch process.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Counters {
    rpc_requests: Option<f64>,
    decoded_rows: Option<f64>,
    cpu_seconds: Option<f64>,
}

impl Counters {
    fn of(metrics: &BTreeMap<String, f64>) -> Self {
        let get = |name: &str| metrics.get(name).copied();
        Self {
            rpc_requests: get("nuthatch_rpc_requests_total"),
            decoded_rows: get("nuthatch_rows_decoded_total"),
            cpu_seconds: get("nuthatch_process_cpu_seconds_total"),
        }
    }

    /// One going backwards means a new process. The indexed block is left out because a reorg
    /// legitimately rewinds it.
    fn follows_restart_of(&self, before: &Self) -> bool {
        let fell = |before: Option<f64>, after: Option<f64>| matches!((before, after), (Some(b), Some(a)) if a < b);
        fell(before.rpc_requests, self.rpc_requests)
            || fell(before.decoded_rows, self.decoded_rows)
            || fell(before.cpu_seconds, self.cpu_seconds)
    }
}

/// Polls one nest, remembering its identity and its counters between polls.
#[derive(Debug)]
pub struct Poller {
    client: Client,
    base: String,
    identity: Option<Arc<Identity>>,
    counters: Option<Counters>,
}

impl Poller {
    /// A poller for the nest at `base`, a URL without a trailing slash.
    pub fn new(client: Client, base: impl Into<String>) -> Self {
        Self {
            client,
            base: base.into(),
            identity: None,
            counters: None,
        }
    }

    /// Polls the nest once.
    pub fn poll(&mut self, request: &Request) -> Snapshot {
        let started = Instant::now();
        let base = self.base.as_str();
        let mut identity_problem = None;
        if self.identity.is_none() {
            match self.client.identity(base) {
                Ok(identity) => self.identity = Some(Arc::new(identity)),
                Err(problem) => {
                    // A runtime's root has no catalogue of its own, only a roster of the nests it
                    // mounts, and a `/ready` with no heights in it that would read as a ready nest
                    // at block zero.
                    if problem.0 == "/tables"
                        && let Ok(roster) = self.client.roster(base)
                    {
                        return self.runtime_root(roster, started);
                    }
                    identity_problem = Some(problem);
                }
            }
        }
        let ready = self.client.ready(base);
        let hot_rows = self.client.hot_rows(base);
        let metrics = self.client.metrics(base);

        let mut restarted = false;
        if let Ok(metrics) = &metrics {
            let counters = Counters::of(metrics);
            restarted = self
                .counters
                .is_some_and(|before| counters.follows_restart_of(&before));
            self.counters = Some(counters);
        }
        if restarted {
            // A restart may have come with a new configuration, and so a new catalogue.
            match self.client.identity(base) {
                Ok(identity) => self.identity = Some(Arc::new(identity)),
                Err(problem) => identity_problem = Some(problem),
            }
        }

        let selection = self.identity.as_ref().and_then(|identity| {
            let wanted = request.table.as_deref()?;
            if identity.sql != SqlAccess::Open {
                return None;
            }
            let table = identity
                .tables
                .tables
                .iter()
                .find(|table| table.table == wanted)?;
            let preview = Preview::new(table, request.rows)?;
            Some(self.client.preview(base, &preview))
        });

        Snapshot {
            taken: Instant::now(),
            elapsed: started.elapsed(),
            ready,
            metrics,
            hot_rows,
            identity: self.identity.clone(),
            identity_problem,
            roster: None,
            selection,
            restarted,
        }
    }

    fn runtime_root(&self, roster: Roster, started: Instant) -> Snapshot {
        let not_a_nest = || Error::Refused("a runtime root, not a nest".into());
        Snapshot {
            taken: Instant::now(),
            elapsed: started.elapsed(),
            ready: Err(not_a_nest()),
            metrics: Err(not_a_nest()),
            hot_rows: Err(not_a_nest()),
            identity: None,
            identity_problem: None,
            roster: Some(roster),
            selection: None,
            restarted: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(rpc: f64, rows: f64, cpu: f64) -> Counters {
        Counters {
            rpc_requests: Some(rpc),
            decoded_rows: Some(rows),
            cpu_seconds: Some(cpu),
        }
    }

    #[test]
    fn a_counter_going_backwards_is_a_restart() {
        let before = counters(100.0, 50.0, 9.5);
        assert!(counters(3.0, 50.0, 9.6).follows_restart_of(&before));
        assert!(counters(100.0, 0.0, 9.6).follows_restart_of(&before));
        assert!(counters(101.0, 50.0, 0.2).follows_restart_of(&before));
    }

    #[test]
    fn counters_standing_still_or_rising_are_not() {
        let before = counters(100.0, 50.0, 9.5);
        assert!(!before.follows_restart_of(&before));
        assert!(!counters(140.0, 80.0, 11.0).follows_restart_of(&before));
    }

    #[test]
    fn a_counter_that_was_never_published_decides_nothing() {
        let before = Counters::default();
        assert!(!counters(1.0, 1.0, 1.0).follows_restart_of(&before));
        assert!(!before.follows_restart_of(&counters(1.0, 1.0, 1.0)));
    }
}
