//! `NestStatus`: the health and heights of one nest.

/// The bridge for [`NestStatus`](qobject::NestStatus).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// `QString` from `cxx_qt_lib`.
        type QString = cxx_qt_lib::QString;
    }

    /// The header marker. A Qt enum, so QML compares `NestStatus.Live` and not a string.
    #[qenum(NestStatus)]
    enum State {
        /// No `/ready` has been read yet.
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

    extern "RustQt" {
        /// The health and heights of one nest.
        ///
        /// - **Owner:** QML. The Rust struct lives inside the C++ object and drops with it.
        /// - **Thread:** the GUI thread, for every property, signal and invokable.
        /// - **Crosses the boundary:** `QString`, integers, a double and an enum, all by value.
        /// - **Worker:** none of its own. It subscribes to the nest's shared poller, which holds
        ///   this object's queue handle and nothing else of it.
        ///
        /// Every property is read-only and they all change together, once per poll, so they share
        /// one notify signal. Heights are `-1` where the nest has none to give: a cursorless role
        /// has no tip, and nought would claim "at block zero".
        #[qobject]
        #[qml_element]
        #[qproperty(QString, url, READ, NOTIFY = changed)]
        #[qproperty(State, state, READ, NOTIFY = changed)]
        #[qproperty(bool, partial, READ, NOTIFY = changed)]
        #[qproperty(bool, insecure, READ, NOTIFY = changed)]
        #[qproperty(QString, version, READ, NOTIFY = changed)]
        #[qproperty(QString, nest_name, cxx_name = "nestName", READ, NOTIFY = changed)]
        #[qproperty(QString, chain, READ, NOTIFY = changed)]
        #[qproperty(i64, tip, READ, NOTIFY = changed)]
        #[qproperty(i64, indexed, READ, NOTIFY = changed)]
        #[qproperty(i64, sealed, READ, NOTIFY = changed)]
        #[qproperty(i64, seal_gap, cxx_name = "sealGap", READ, NOTIFY = changed)]
        #[qproperty(i64, lag_blocks, cxx_name = "lagBlocks", READ, NOTIFY = changed)]
        #[qproperty(i64, hot_rows, cxx_name = "hotRows", READ, NOTIFY = changed)]
        #[qproperty(i32, poll_interval_secs, cxx_name = "pollIntervalSecs", READ, NOTIFY = changed)]
        #[qproperty(i32, refresh_ms, cxx_name = "refreshMs", READ, NOTIFY = changed)]
        #[qproperty(f64, sync_fraction, cxx_name = "syncFraction", READ, NOTIFY = changed)]
        #[qproperty(QString, sync_label, cxx_name = "syncLabel", READ, NOTIFY = changed)]
        #[qproperty(QString, problems, READ, NOTIFY = changed)]
        type NestStatus = super::NestStatusRust;

        /// Some property changed.
        #[qsignal]
        fn changed(self: Pin<&mut NestStatus>);

        /// The nest is a new process since the last poll.
        #[qsignal]
        #[cxx_name = "restartSeen"]
        fn restart_seen(self: Pin<&mut NestStatus>);

        /// `/ready` could not be read, or the URL was refused.
        #[qsignal]
        #[cxx_name = "pollFailed"]
        fn poll_failed(self: Pin<&mut NestStatus>, message: &QString);

        /// Starts showing the nest at `url`, in place of any shown before.
        #[qinvokable]
        #[cxx_name = "connectTo"]
        fn connect_to(self: Pin<&mut NestStatus>, url: &QString);

        /// Polls now rather than at the next interval.
        #[qinvokable]
        fn refresh(self: Pin<&mut NestStatus>);
    }

    impl cxx_qt::Threading for NestStatus {}
}

use std::{pin::Pin, sync::Arc};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use desk_core::{
    feed::{Delivery, Hub, Sink, Subscription},
    status::{self, Status},
};
use nest_client::{config::check_url, poll::Snapshot};

use crate::guard::{contained, to_i64};
use qobject::State;

impl Default for State {
    fn default() -> Self {
        Self::Connecting
    }
}

impl From<status::State> for State {
    fn from(state: status::State) -> Self {
        match state {
            status::State::Connecting => Self::Connecting,
            status::State::Live => Self::Live,
            status::State::Attention => Self::Attention,
            status::State::Backfill => Self::Backfill,
            status::State::Quarantined => Self::Quarantined,
            status::State::Stale => Self::Stale,
        }
    }
}

/// The Rust state behind [`qobject::NestStatus`].
pub struct NestStatusRust {
    url: QString,
    state: State,
    partial: bool,
    insecure: bool,
    version: QString,
    nest_name: QString,
    chain: QString,
    tip: i64,
    indexed: i64,
    sealed: i64,
    seal_gap: i64,
    lag_blocks: i64,
    hot_rows: i64,
    poll_interval_secs: i32,
    refresh_ms: i32,
    sync_fraction: f64,
    sync_label: QString,
    problems: QString,

    status: Status,
    /// Dropping this is all destruction does: it takes this object's sink out of the poller's list
    /// and returns at once. The poller is never joined.
    subscription: Option<Subscription>,
    /// Counts calls to `connectTo`, so a snapshot queued for an earlier URL is not applied.
    generation: u64,
}

impl Default for NestStatusRust {
    fn default() -> Self {
        Self {
            url: QString::default(),
            state: State::Connecting,
            partial: false,
            insecure: false,
            version: QString::default(),
            nest_name: QString::default(),
            chain: QString::default(),
            tip: -1,
            indexed: 0,
            sealed: 0,
            seal_gap: 0,
            lag_blocks: -1,
            hot_rows: -1,
            poll_interval_secs: 0,
            refresh_ms: 0,
            sync_fraction: 0.0,
            sync_label: QString::default(),
            problems: QString::default(),
            status: Status::default(),
            subscription: None,
            generation: 0,
        }
    }
}

impl qobject::NestStatus {
    fn connect_to(mut self: Pin<&mut Self>, url: &QString) {
        contained("NestStatus.connectTo", || {
            let generation = self.generation + 1;
            {
                let mut rust = self.as_mut().rust_mut();
                rust.generation = generation;
                rust.subscription = None;
                rust.status = Status::default();
            }
            let endpoint = match check_url(&url.to_string()) {
                Ok(endpoint) => endpoint,
                Err(problem) => return self.refuse(&url.to_string(), &problem.to_string()),
            };
            let hub = match Hub::global() {
                Ok(hub) => hub,
                Err(error) => return self.refuse(&endpoint.url, &error.to_string()),
            };
            {
                let mut rust = self.as_mut().rust_mut();
                rust.url = QString::from(&endpoint.url);
                rust.insecure = endpoint.plain_remote;
            }
            self.as_mut().publish();

            let thread = self.qt_thread();
            let sink: Sink = Box::new(move |snapshot| {
                let snapshot = Arc::clone(snapshot);
                let queued = thread.queue(move |status| {
                    contained("NestStatus's update", || {
                        status.apply(generation, &snapshot)
                    });
                });
                // The queue refuses once the QObject is destroyed, which is how the poller learns.
                match queued {
                    Ok(()) => Delivery::Taken,
                    Err(_) => {
                        #[cfg(feature = "lifetime-probe")]
                        crate::probe::count_refusal();
                        Delivery::Gone
                    }
                }
            });
            let subscription = hub.subscribe(&endpoint.url, sink);
            #[cfg(feature = "lifetime-probe")]
            if crate::probe::take_leak() {
                std::mem::forget(subscription);
                return;
            }
            self.as_mut().rust_mut().subscription = Some(subscription);
        });
    }

    fn refresh(self: Pin<&mut Self>) {
        contained("NestStatus.refresh", || {
            if let Some(subscription) = &self.subscription {
                subscription.refresh();
            }
        });
    }

    /// Shows `url` as refused for `reason`.
    fn refuse(mut self: Pin<&mut Self>, url: &str, reason: &str) {
        {
            let mut rust = self.as_mut().rust_mut();
            rust.url = QString::from(url);
            rust.insecure = false;
        }
        self.as_mut().publish();
        let message = QString::from(reason);
        self.as_mut().rust_mut().problems = message.clone();
        self.as_mut().changed();
        self.poll_failed(&message);
    }

    /// Runs on the GUI thread, queued by the poller.
    fn apply(mut self: Pin<&mut Self>, generation: u64, snapshot: &Snapshot) {
        if generation != self.generation {
            return;
        }
        let outcome = self.as_mut().rust_mut().status.apply(snapshot);
        self.as_mut().publish();
        if outcome.restarted {
            self.as_mut().restart_seen();
        }
        if let Some(message) = outcome.poll_failed {
            self.poll_failed(&QString::from(&message));
        }
    }

    /// Copies the folded status into the properties and says they changed.
    fn publish(mut self: Pin<&mut Self>) {
        let view = self.status.view();
        let height = |value: Option<u64>| value.map_or(-1, to_i64);
        {
            let mut rust = self.as_mut().rust_mut();
            rust.state = view.state.into();
            rust.partial = view.partial;
            rust.version = QString::from(&view.version);
            rust.nest_name = QString::from(&view.nest_name);
            rust.chain = QString::from(&view.chain);
            rust.tip = height(view.tip);
            rust.indexed = to_i64(view.indexed);
            rust.sealed = to_i64(view.sealed);
            rust.seal_gap = to_i64(view.seal_gap);
            rust.lag_blocks = height(view.lag_blocks);
            rust.hot_rows = height(view.hot_rows);
            rust.poll_interval_secs = i32::try_from(view.poll_interval_secs).unwrap_or(i32::MAX);
            rust.refresh_ms = i32::try_from(view.refresh_ms).unwrap_or(i32::MAX);
            rust.sync_fraction = view.sync_fraction;
            rust.sync_label = QString::from(&view.sync_label);
            rust.problems = QString::from(&view.problems);
        }
        self.changed();
    }
}
