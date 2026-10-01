//! One nest's health, folded from the polls that have arrived so far.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use nest_client::{poll::Snapshot, types::Ready};

use crate::format::{count_blocks, format_span, group_digits};

/// The poll interval used until the nest says how often it polls.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// Tips kept for the chain's block rate.
const TIP_HISTORY: usize = 64;

/// How often to poll a nest that polls its RPC every `nest` seconds: as often as it does, within
/// two to thirty seconds.
pub fn poll_interval(nest: Option<u64>) -> Duration {
    nest.filter(|secs| *secs > 0)
        .map_or(DEFAULT_POLL_INTERVAL, |secs| {
            Duration::from_secs(secs).clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL)
        })
}

/// The header marker, as the terminal client words it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
    /// No `/ready` has been read yet.
    #[default]
    Connecting,
    /// Ready, and nothing stalled.
    Live,
    /// The nest answered and something is wrong with it.
    Attention,
    /// A backfill pass is running.
    Backfill,
    /// The runtime has set the nest aside.
    Quarantined,
    /// The last `/ready` failed: every figure is from an earlier poll.
    Stale,
}

/// What a status display shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// The header marker.
    pub state: State,
    /// The nest answered but some of what the screen shows could not be fetched.
    pub partial: bool,
    /// The nest's version, or empty.
    pub version: String,
    /// The authored nest name, or empty.
    pub nest_name: String,
    /// The chain, or empty.
    pub chain: String,
    /// Chain tip. `None` for a cursorless role or before the first poll.
    pub tip: Option<u64>,
    /// Last block indexed.
    pub indexed: u64,
    /// Last block sealed.
    pub sealed: u64,
    /// Blocks indexed and not yet sealed.
    pub seal_gap: u64,
    /// Blocks behind the tip. `None` for a cursorless role or before the first poll.
    pub lag_blocks: Option<u64>,
    /// Seconds between polls.
    pub poll_interval_secs: u64,
    /// Rows in the hot store, where the nest says.
    pub hot_rows: Option<u64>,
    /// How long the last poll took, in milliseconds.
    pub refresh_ms: u64,
    /// The sync gauge's fill, from nought to one.
    pub sync_fraction: f64,
    /// The sync gauge's label.
    pub sync_label: String,
    /// One line on what is wrong, or empty.
    pub problems: String,
}

/// What applying one poll set off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The nest restarted since the poll before.
    pub restarted: bool,
    /// `/ready` failed, and why.
    pub poll_failed: Option<String>,
}

struct Backfill {
    origin: u64,
    current: u64,
    target: u64,
}

/// One nest's health.
#[derive(Debug, Default)]
pub struct Status {
    ready: Option<Ready>,
    ready_failed: bool,
    backfill_gauges: Option<(u64, u64, u64)>,
    problems: Vec<(&'static str, String)>,
    tips: VecDeque<(Instant, u64)>,
    nest_name: String,
    chain: String,
    hot_rows: Option<u64>,
    refresh_ms: u64,
}

impl Status {
    /// Folds one poll in.
    pub fn apply(&mut self, snapshot: &Snapshot) -> Outcome {
        let mut problems = Vec::new();
        let mut outcome = Outcome {
            restarted: snapshot.restarted,
            poll_failed: None,
        };
        if let Some(roster) = &snapshot.roster {
            let mounts: Vec<String> = roster.nests.iter().map(|nest| nest.path()).collect();
            let message = format!(
                "this is a runtime's root, not a nest. It mounts {}: point at one of those",
                mounts.join(", ")
            );
            self.ready_failed = true;
            self.problems = vec![("/nests", message.clone())];
            outcome.poll_failed = Some(message);
            return outcome;
        }
        if let Some((endpoint, error)) = &snapshot.identity_problem {
            problems.push((*endpoint, error.to_string()));
        }
        if let Some(identity) = &snapshot.identity {
            self.nest_name = identity.nest_name.clone().unwrap_or_default();
            self.chain = identity.chain.clone().unwrap_or_default();
        }
        match &snapshot.ready {
            Ok(ready) => {
                if snapshot.restarted {
                    self.tips.clear();
                }
                if let Some(tip) = ready.tip {
                    self.tips.push_back((snapshot.taken, tip));
                    if self.tips.len() > TIP_HISTORY {
                        self.tips.pop_front();
                    }
                }
                self.ready = Some(ready.clone());
                self.ready_failed = false;
            }
            Err(error) => {
                problems.push(("/ready", error.to_string()));
                self.ready_failed = true;
                outcome.poll_failed = Some(error.to_string());
            }
        }
        match &snapshot.metrics {
            Ok(metrics) => {
                // Nuthatch before 3.4 published the backfill pass as gauges rather than on `/ready`.
                let gauge = |name: &str| metrics.get(name).copied().unwrap_or_default() as u64;
                self.backfill_gauges = (gauge("nuthatch_direct_backfill_active") != 0).then(|| {
                    (
                        gauge("nuthatch_direct_backfill_from_block"),
                        gauge("nuthatch_direct_backfill_current_block"),
                        gauge("nuthatch_direct_backfill_target_block"),
                    )
                });
            }
            Err(error) => {
                self.backfill_gauges = None;
                problems.push(("/metrics", error.to_string()));
            }
        }
        match &snapshot.hot_rows {
            Ok(rows) => self.hot_rows = *rows,
            Err(error) => {
                self.hot_rows = None;
                problems.push(("/", error.to_string()));
            }
        }
        if let Some(Err(error)) = &snapshot.selection {
            problems.push(("/sql", error.to_string()));
        }
        self.refresh_ms = snapshot.elapsed.as_millis() as u64;
        self.problems = problems;
        outcome
    }

    fn backfill(&self) -> Option<Backfill> {
        let ready = self.ready.as_ref()?;
        if ready.seal_direct_active {
            return Some(Backfill {
                origin: ready.seal_direct_origin.unwrap_or_default(),
                current: ready.seal_direct_completed.unwrap_or_default(),
                target: ready.seal_direct_target.unwrap_or_default(),
            });
        }
        self.backfill_gauges
            .map(|(origin, current, target)| Backfill {
                origin,
                current,
                target,
            })
    }

    fn state(&self) -> State {
        let Some(ready) = self.ready.as_ref() else {
            return State::Connecting;
        };
        if self.ready_failed {
            State::Stale
        } else if ready.quarantined {
            State::Quarantined
        } else if self.backfill().is_some() {
            State::Backfill
        } else if ready.ready
            && !ready.stalled
            && !ready.wedged
            && !ready.initial_poll_failed
            && !ready.seal_direct_stalled
            && !ready.entities_stalled
        {
            State::Live
        } else {
            State::Attention
        }
    }

    fn chain_block_rate(&self) -> Option<f64> {
        let (&(first_at, first_tip), &(last_at, last_tip)) =
            (self.tips.front()?, self.tips.back()?);
        let blocks = last_tip.checked_sub(first_tip)?;
        let seconds = last_at.saturating_duration_since(first_at).as_secs_f64();
        (blocks > 0 && seconds > 0.0).then(|| blocks as f64 / seconds)
    }

    /// The gauge's fill and label. Lag is measured against the least the nest can be expected to
    /// trail by, one poll interval's worth of blocks or one block, whichever is more: full within
    /// that, half at twice it.
    fn sync(&self, interval: Duration) -> (f64, String) {
        let Some(ready) = self.ready.as_ref() else {
            return (0.0, "waiting for /ready".into());
        };
        if let Some(backfill) = self.backfill() {
            let span = backfill.target.saturating_sub(backfill.origin).max(1);
            let done = backfill.current.saturating_sub(backfill.origin).min(span);
            return (
                done as f64 / span as f64,
                format!(
                    "{} / {}",
                    group_digits(backfill.current),
                    group_digits(backfill.target)
                ),
            );
        }
        let (Some(_), Some(lag)) = (ready.tip, ready.lag_blocks) else {
            return (0.0, "cursorless".into());
        };
        if lag == 0 {
            return (1.0, "at tip".into());
        }
        let rate = self.chain_block_rate();
        let step = rate.map_or(1.0, |rate| (rate * interval.as_secs_f64()).max(1.0));
        let label = match rate {
            Some(rate) => format!(
                "{} · {} behind",
                count_blocks(lag),
                format_span(Duration::from_secs_f64(lag as f64 / rate))
            ),
            None => format!("{} behind", count_blocks(lag)),
        };
        ((step / lag as f64).min(1.0), label)
    }

    /// What to show now.
    pub fn view(&self) -> View {
        let ready = self.ready.as_ref();
        let interval = poll_interval(
            ready
                .and_then(|ready| ready.freshness.as_ref())
                .and_then(|freshness| freshness.poll_interval_secs),
        );
        let (sync_fraction, sync_label) = self.sync(interval);
        let state = self.state();
        View {
            state,
            partial: !self.problems.is_empty()
                && !matches!(state, State::Connecting | State::Stale),
            version: ready
                .and_then(|ready| ready.version.clone())
                .unwrap_or_default(),
            nest_name: self.nest_name.clone(),
            chain: self.chain.clone(),
            tip: ready.and_then(|ready| ready.tip),
            indexed: ready.map_or(0, |ready| ready.last_block),
            sealed: ready.map_or(0, |ready| ready.sealed_through),
            seal_gap: ready.map_or(0, |ready| {
                ready.last_block.saturating_sub(ready.sealed_through)
            }),
            lag_blocks: ready.and_then(|ready| ready.lag_blocks),
            poll_interval_secs: interval.as_secs(),
            hot_rows: self.hot_rows,
            refresh_ms: self.refresh_ms,
            sync_fraction,
            sync_label,
            problems: self
                .problems
                .iter()
                .map(|(endpoint, error)| format!("{endpoint}: {error}"))
                .collect::<Vec<_>>()
                .join("  ·  "),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use nest_client::{
        Error,
        types::{Freshness, Roster},
    };

    use super::*;

    pub(crate) fn ready(tip: u64, last_block: u64) -> Ready {
        Ready {
            ready: true,
            tip: Some(tip),
            lag_blocks: Some(tip - last_block),
            last_block,
            freshness: Some(Freshness {
                poll_interval_secs: Some(12),
            }),
            version: Some("3.13.3".into()),
            ..Ready::default()
        }
    }

    pub(crate) fn snapshot(at: Instant, ready: Result<Ready, Error>) -> Snapshot {
        Snapshot {
            taken: at,
            elapsed: Duration::from_millis(40),
            ready,
            metrics: Ok(BTreeMap::new()),
            hot_rows: Ok(Some(17_538)),
            identity: None,
            identity_problem: None,
            roster: None,
            selection: None,
            restarted: false,
        }
    }

    fn state_of(ready: Ready) -> State {
        let mut status = Status::default();
        status.apply(&snapshot(Instant::now(), Ok(ready)));
        status.view().state
    }

    #[test]
    fn before_any_poll_it_is_connecting() {
        let view = Status::default().view();
        assert_eq!(view.state, State::Connecting);
        assert_eq!(view.tip, None);
        assert_eq!(view.sync_label, "waiting for /ready");
        assert_eq!(view.poll_interval_secs, 5);
        assert!(!view.partial);
    }

    #[test]
    fn a_ready_nest_is_live_with_its_heights() {
        let mut status = Status::default();
        let outcome = status.apply(&snapshot(Instant::now(), Ok(ready(26_096_543, 26_096_410))));
        assert_eq!(outcome, Outcome::default());
        let view = status.view();
        assert_eq!(view.state, State::Live);
        assert_eq!(view.tip, Some(26_096_543));
        assert_eq!(view.indexed, 26_096_410);
        assert_eq!(view.lag_blocks, Some(133));
        assert_eq!(view.seal_gap, 26_096_410);
        assert_eq!(view.version, "3.13.3");
        assert_eq!(view.poll_interval_secs, 12);
        assert_eq!(view.hot_rows, Some(17_538));
        assert_eq!(view.refresh_ms, 40);
        assert_eq!(view.sync_label, "133 blocks behind");
    }

    #[test]
    fn each_stall_the_nest_reports_is_attention() {
        let healthy = ready(100, 100);
        assert_eq!(state_of(healthy.clone()), State::Live);
        for sick in [
            Ready {
                ready: false,
                ..healthy.clone()
            },
            Ready {
                stalled: true,
                ..healthy.clone()
            },
            Ready {
                wedged: true,
                ..healthy.clone()
            },
            Ready {
                initial_poll_failed: true,
                ..healthy.clone()
            },
            Ready {
                seal_direct_stalled: true,
                ..healthy.clone()
            },
            Ready {
                entities_stalled: true,
                ..healthy.clone()
            },
        ] {
            assert_eq!(state_of(sick), State::Attention);
        }
    }

    #[test]
    fn quarantine_outranks_backfill_which_outranks_attention() {
        let backfilling = Ready {
            seal_direct_active: true,
            seal_direct_origin: Some(100),
            seal_direct_completed: Some(150),
            seal_direct_target: Some(300),
            stalled: true,
            ..ready(400, 100)
        };
        assert_eq!(state_of(backfilling.clone()), State::Backfill);
        assert_eq!(
            state_of(Ready {
                quarantined: true,
                ..backfilling
            }),
            State::Quarantined
        );
    }

    #[test]
    fn a_backfill_fills_the_gauge_by_blocks_done() {
        let mut status = Status::default();
        status.apply(&snapshot(
            Instant::now(),
            Ok(Ready {
                seal_direct_active: true,
                seal_direct_origin: Some(1_000),
                seal_direct_completed: Some(1_500),
                seal_direct_target: Some(3_000),
                ..ready(4_000, 1_000)
            }),
        ));
        let view = status.view();
        assert_eq!(view.sync_fraction, 0.25);
        assert_eq!(view.sync_label, "1,500 / 3,000");
    }

    #[test]
    fn an_older_nest_reports_its_backfill_as_gauges() {
        let mut status = Status::default();
        let mut poll = snapshot(Instant::now(), Ok(ready(4_000, 1_000)));
        poll.metrics = Ok(BTreeMap::from([
            ("nuthatch_direct_backfill_active".to_owned(), 1.0),
            ("nuthatch_direct_backfill_from_block".to_owned(), 0.0),
            ("nuthatch_direct_backfill_current_block".to_owned(), 50.0),
            ("nuthatch_direct_backfill_target_block".to_owned(), 100.0),
        ]));
        status.apply(&poll);
        assert_eq!(status.view().state, State::Backfill);
        assert_eq!(status.view().sync_fraction, 0.5);
    }

    #[test]
    fn a_failed_ready_keeps_the_last_figures_and_says_they_are_stale() {
        let mut status = Status::default();
        status.apply(&snapshot(Instant::now(), Ok(ready(500, 490))));
        let outcome = status.apply(&snapshot(Instant::now(), Err(Error::Connect)));
        assert_eq!(outcome.poll_failed.as_deref(), Some("cannot connect"));
        let view = status.view();
        assert_eq!(view.state, State::Stale);
        assert_eq!(view.indexed, 490);
        assert_eq!(view.problems, "/ready: cannot connect");
        // Stale already says the figures cannot be trusted; partial would say it twice.
        assert!(!view.partial);
    }

    #[test]
    fn a_nest_never_reached_stays_connecting() {
        let mut status = Status::default();
        status.apply(&snapshot(Instant::now(), Err(Error::Connect)));
        let view = status.view();
        assert_eq!(view.state, State::Connecting);
        assert_eq!(view.problems, "/ready: cannot connect");
    }

    #[test]
    fn a_panel_that_could_not_be_fetched_makes_it_partial() {
        let mut status = Status::default();
        let mut poll = snapshot(Instant::now(), Ok(ready(500, 500)));
        poll.metrics = Err(Error::Status(404));
        poll.hot_rows = Err(Error::Timeout);
        status.apply(&poll);
        let view = status.view();
        assert_eq!(view.state, State::Live);
        assert!(view.partial);
        assert_eq!(view.problems, "/metrics: HTTP 404  ·  /: timed out");
        assert_eq!(view.hot_rows, None);

        status.apply(&snapshot(Instant::now(), Ok(ready(501, 501))));
        assert!(!status.view().partial);
    }

    #[test]
    fn a_cursorless_role_is_not_shown_at_block_zero() {
        let mut status = Status::default();
        status.apply(&snapshot(
            Instant::now(),
            Ok(Ready {
                tip: None,
                lag_blocks: None,
                ..ready(1, 1)
            }),
        ));
        let view = status.view();
        assert_eq!(view.tip, None);
        assert_eq!(view.lag_blocks, None);
        assert_eq!(view.sync_label, "cursorless");
    }

    #[test]
    fn at_tip_the_gauge_is_full() {
        let mut status = Status::default();
        status.apply(&snapshot(Instant::now(), Ok(ready(700, 700))));
        assert_eq!(status.view().sync_fraction, 1.0);
        assert_eq!(status.view().sync_label, "at tip");
    }

    #[test]
    fn lag_is_worded_in_time_once_the_chains_pace_is_known() {
        let start = Instant::now();
        let mut status = Status::default();
        status.apply(&snapshot(start, Ok(ready(1_000, 880))));
        // Ten blocks in 120 seconds: a block every twelve.
        status.apply(&snapshot(
            start + Duration::from_secs(120),
            Ok(ready(1_010, 890)),
        ));
        let view = status.view();
        assert_eq!(view.sync_label, "120 blocks · 24m behind");
        // One poll interval is one block at this pace, and the nest is 120 behind.
        assert!((view.sync_fraction - 1.0 / 120.0).abs() < 1e-9);
    }

    #[test]
    fn a_restart_is_passed_on_and_forgets_the_chains_pace() {
        let start = Instant::now();
        let mut status = Status::default();
        status.apply(&snapshot(start, Ok(ready(1_000, 880))));
        let mut poll = snapshot(start + Duration::from_secs(120), Ok(ready(1_010, 890)));
        poll.restarted = true;
        assert!(status.apply(&poll).restarted);
        assert_eq!(status.view().sync_label, "120 blocks behind");
    }

    #[test]
    fn a_runtime_root_says_what_to_point_at_instead() {
        let mut status = Status::default();
        let mut poll = snapshot(Instant::now(), Err(Error::Refused("a runtime root".into())));
        poll.roster = Some(roster(&["usdc", "dai"]));
        let outcome = status.apply(&poll);
        let message = outcome.poll_failed.unwrap();
        assert!(message.contains("It mounts /usdc, /dai"), "{message}");
        assert_eq!(status.view().state, State::Connecting);
    }

    fn roster(names: &[&str]) -> Roster {
        Roster {
            nests: names
                .iter()
                .map(|name| nest_client::types::RosterNest {
                    name: (*name).to_owned(),
                    base_path: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_poll_interval_follows_the_nest_within_bounds() {
        assert_eq!(poll_interval(None), Duration::from_secs(5));
        assert_eq!(poll_interval(Some(0)), Duration::from_secs(5));
        assert_eq!(poll_interval(Some(1)), Duration::from_secs(2));
        assert_eq!(poll_interval(Some(12)), Duration::from_secs(12));
        assert_eq!(poll_interval(Some(600)), Duration::from_secs(30));
    }
}
