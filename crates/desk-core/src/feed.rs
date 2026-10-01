//! One polling thread per nest, shared by everything that shows that nest.
//!
//! A QObject cannot hold another QObject, and a worker must not hold one at all. So the things that
//! show a nest never meet: each subscribes here with a [`Sink`], a closure that takes an owned
//! snapshot and posts it to wherever it lives. The worker knows the nest's URL and a list of sinks.
//! A sink that answers [`Delivery::Gone`] is dropped, and a worker with no sinks left exits.
//!
//! Nothing here blocks the caller for longer than a lock held across a few closure calls. In
//! particular dropping a [`Subscription`] never waits for the worker, which may be in the middle of
//! a request: it finishes that request, finds nobody listening, and goes.

use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use nest_client::{
    Client, Error, Limits,
    poll::{Poller, Request, Snapshot},
};

use crate::status::poll_interval;

/// How long one request to a nest may take before the poll moves on.
const POLL_TIMEOUT: Duration = Duration::from_secs(10);
/// Newest rows fetched for the selected table.
pub const FEED_ROWS: usize = 50;

/// Whether a sink is still there to deliver to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The snapshot was handed on.
    Taken,
    /// The receiver has been destroyed. The sink is dropped and never called again.
    Gone,
}

/// Where snapshots go. Called on the worker thread, and once on the subscribing thread if a
/// snapshot is already to hand, so it must only post the snapshot on, never act on it.
pub type Sink = Box<dyn FnMut(&Arc<Snapshot>) -> Delivery + Send>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not take every later poll down with it.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Control {
    sinks: Vec<(u64, Sink)>,
    next_id: u64,
    last: Option<Arc<Snapshot>>,
    request: Request,
    refresh: bool,
}

struct Feed {
    url: String,
    control: Mutex<Control>,
    wake: Condvar,
}

struct Shared {
    client: Client,
    feeds: Mutex<HashMap<String, Arc<Feed>>>,
    workers: AtomicUsize,
}

/// The polling threads, one per nest URL.
#[derive(Clone)]
pub struct Hub {
    shared: Arc<Shared>,
}

/// A place in a nest's list of sinks. Dropping it takes the sink out.
pub struct Subscription {
    feed: Arc<Feed>,
    id: u64,
}

impl Hub {
    /// A hub whose workers fetch with `client`.
    pub fn new(client: Client) -> Self {
        Self {
            shared: Arc::new(Shared {
                client,
                feeds: Mutex::default(),
                workers: AtomicUsize::new(0),
            }),
        }
    }

    /// The process's hub, or why its HTTP client could not be built.
    pub fn global() -> Result<&'static Hub, Error> {
        static HUB: OnceLock<Result<Hub, Error>> = OnceLock::new();
        HUB.get_or_init(|| Client::new(POLL_TIMEOUT, Limits::default()).map(Hub::new))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Worker threads alive now.
    pub fn workers(&self) -> usize {
        self.shared.workers.load(Ordering::SeqCst)
    }

    /// Starts sending the nest at `url` to `sink`, starting its worker if this is the first sink.
    ///
    /// If the nest has been polled already, `sink` is called with the last snapshot before this
    /// returns, so a late subscriber does not wait a poll interval to show anything.
    pub fn subscribe(&self, url: &str, mut sink: Sink) -> Subscription {
        // Held across the whole call so a worker cannot retire the feed between finding it and
        // joining it. A retiring worker takes this lock first too.
        let mut feeds = lock(&self.shared.feeds);
        let feed = feeds.entry(url.to_owned()).or_insert_with(|| {
            let feed = Arc::new(Feed {
                url: url.to_owned(),
                control: Mutex::new(Control {
                    sinks: Vec::new(),
                    next_id: 0,
                    last: None,
                    request: Request {
                        table: None,
                        rows: FEED_ROWS,
                    },
                    refresh: false,
                }),
                wake: Condvar::new(),
            });
            let (shared, worker_feed) = (Arc::clone(&self.shared), Arc::clone(&feed));
            shared.workers.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                run(&shared, &worker_feed);
                shared.workers.fetch_sub(1, Ordering::SeqCst);
            });
            feed
        });
        let feed = Arc::clone(feed);
        let mut control = lock(&feed.control);
        let id = control.next_id;
        control.next_id += 1;
        let delivered = match &control.last {
            Some(last) => sink(last),
            None => Delivery::Taken,
        };
        if delivered == Delivery::Taken {
            control.sinks.push((id, sink));
        }
        drop(control);
        Subscription { feed, id }
    }
}

impl Subscription {
    /// The URL polled.
    pub fn url(&self) -> &str {
        &self.feed.url
    }

    /// Polls now rather than at the next interval.
    pub fn refresh(&self) {
        lock(&self.feed.control).refresh = true;
        self.feed.wake.notify_all();
    }

    /// Previews `table` from the next poll on, and polls now. The selection belongs to the nest,
    /// not the subscriber: there is one table list per nest.
    pub fn select(&self, table: Option<String>) {
        let mut control = lock(&self.feed.control);
        if control.request.table != table {
            control.request.table = table;
            drop(control);
            self.feed.wake.notify_all();
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        // The sink is taken out under the lock and dropped after it, so whatever the sink owns is
        // not destroyed while the worker is locked out.
        let sink = {
            let mut control = lock(&self.feed.control);
            let at = control.sinks.iter().position(|(id, _)| *id == self.id);
            at.map(|at| control.sinks.swap_remove(at))
        };
        drop(sink);
        self.feed.wake.notify_all();
    }
}

/// The worker: poll, deliver, wait, until nobody is listening.
fn run(shared: &Shared, feed: &Feed) {
    let mut poller = Poller::new(shared.client.clone(), feed.url.clone());
    loop {
        let request = {
            // The order every thread takes these two locks in: feeds, then control.
            let mut feeds = lock(&shared.feeds);
            let control = lock(&feed.control);
            if control.sinks.is_empty() {
                // Only this feed is removed: a later subscriber to the URL may already have
                // replaced it.
                if feeds
                    .get(&feed.url)
                    .is_some_and(|current| std::ptr::eq(Arc::as_ptr(current), feed))
                {
                    feeds.remove(&feed.url);
                }
                return;
            }
            control.request.clone()
        };

        // A panic in a poll costs that poll, not the thread and not the process.
        let polled = catch_unwind(AssertUnwindSafe(|| poller.poll(&request)));
        let interval = match &polled {
            Ok(snapshot) => poll_interval(
                snapshot
                    .ready
                    .as_ref()
                    .ok()
                    .and_then(|ready| ready.freshness.as_ref())
                    .and_then(|freshness| freshness.poll_interval_secs),
            ),
            Err(_) => poll_interval(None),
        };

        let mut control = lock(&feed.control);
        if let Ok(snapshot) = polled {
            let snapshot = Arc::new(snapshot);
            control
                .sinks
                .retain_mut(|(_, sink)| sink(&snapshot) == Delivery::Taken);
            control.last = Some(snapshot);
        }
        control.refresh = false;
        let deadline = Instant::now() + interval;
        while !control.sinks.is_empty() && !control.refresh && control.request == request {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            control = feed
                .wake
                .wait_timeout(control, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};

    use mock_nest::{MockNest, fixtures};

    use super::*;

    fn hub() -> Hub {
        Hub::new(Client::new(Duration::from_secs(5), Limits::default()).unwrap())
    }

    /// A sink that forwards to a channel and is gone once the receiver is.
    fn channel() -> (Sink, Receiver<Arc<Snapshot>>) {
        let (sender, receiver) = mpsc::channel();
        let sink: Sink = Box::new(move |snapshot| match sender.send(Arc::clone(snapshot)) {
            Ok(()) => Delivery::Taken,
            Err(_) => Delivery::Gone,
        });
        (sink, receiver)
    }

    fn next(receiver: &Receiver<Arc<Snapshot>>) -> Arc<Snapshot> {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("a snapshot within five seconds")
    }

    fn settles_to_no_workers(hub: &Hub) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.workers() != 0 {
            assert!(
                Instant::now() < deadline,
                "{} worker(s) left",
                hub.workers()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_subscriber_gets_the_nest() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (sink, snapshots) = channel();
        let subscription = hub.subscribe(&nest.url(), sink);
        assert_eq!(subscription.url(), nest.url());
        let snapshot = next(&snapshots);
        assert_eq!(snapshot.ready.as_ref().unwrap().last_block, 26_096_410);
        assert_eq!(hub.workers(), 1);
    }

    #[test]
    fn two_subscribers_to_one_nest_share_one_worker_and_one_poll() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (first, first_snapshots) = channel();
        let (second, second_snapshots) = channel();
        let _first = hub.subscribe(&nest.url(), first);
        let _second = hub.subscribe(&nest.url(), second);
        let (a, b) = (next(&first_snapshots), next(&second_snapshots));
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(hub.workers(), 1);
        assert_eq!(nest.hits("/ready"), 1);
    }

    #[test]
    fn a_late_subscriber_is_shown_the_last_poll_at_once() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (early, early_snapshots) = channel();
        let _early = hub.subscribe(&nest.url(), early);
        let polled = next(&early_snapshots);

        let (late, late_snapshots) = channel();
        let _late = hub.subscribe(&nest.url(), late);
        // Already there when `subscribe` returns: no waiting for the twelve-second interval.
        let replayed = late_snapshots
            .try_recv()
            .expect("the last snapshot, replayed");
        assert!(Arc::ptr_eq(&polled, &replayed));
        assert_eq!(nest.hits("/ready"), 1);
    }

    #[test]
    fn dropping_the_last_subscription_ends_the_worker() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (sink, snapshots) = channel();
        let subscription = hub.subscribe(&nest.url(), sink);
        next(&snapshots);
        drop(subscription);
        settles_to_no_workers(&hub);
    }

    #[test]
    fn dropping_a_subscription_mid_poll_does_not_wait_for_the_poll() {
        let nest = MockNest::recorded();
        nest.delay("/ready", Duration::from_millis(1500));
        let hub = hub();
        let (sink, snapshots) = channel();
        let subscription = hub.subscribe(&nest.url(), sink);
        // Let the worker get as far as the request that will hang.
        while nest.hits("/ready") == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        drop(subscription);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "{:?}",
            started.elapsed()
        );
        // The worker finishes its request, finds nobody listening, and goes.
        settles_to_no_workers(&hub);
        assert!(snapshots.try_recv().is_err());
    }

    #[test]
    fn a_sink_whose_receiver_is_gone_is_dropped_and_the_worker_with_it() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (sink, snapshots) = channel();
        // The subscription is kept alive: this is the receiver dying without unsubscribing, which
        // is what a destroyed QObject looks like from the worker's side.
        let subscription = hub.subscribe(&nest.url(), sink);
        next(&snapshots);
        drop(snapshots);
        subscription.refresh();
        settles_to_no_workers(&hub);
    }

    #[test]
    fn one_nest_going_away_leaves_the_other_polling() {
        let (usdc, dai) = (MockNest::recorded(), MockNest::recorded());
        let hub = hub();
        let (usdc_sink, usdc_snapshots) = channel();
        let (dai_sink, dai_snapshots) = channel();
        let usdc_subscription = hub.subscribe(&usdc.url(), usdc_sink);
        let dai_subscription = hub.subscribe(&dai.url(), dai_sink);
        next(&usdc_snapshots);
        next(&dai_snapshots);
        assert_eq!(hub.workers(), 2);

        drop(usdc_subscription);
        dai_subscription.refresh();
        next(&dai_snapshots);
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.workers() != 1 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn refresh_polls_now() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (sink, snapshots) = channel();
        let subscription = hub.subscribe(&nest.url(), sink);
        next(&snapshots);
        // The recorded nest asks for a twelve-second interval; this arrives well inside it.
        subscription.refresh();
        next(&snapshots);
        assert_eq!(nest.hits("/ready"), 2);
    }

    #[test]
    fn selecting_a_table_polls_now_and_previews_it() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (sink, snapshots) = channel();
        let subscription = hub.subscribe(&nest.url(), sink);
        assert!(next(&snapshots).selection.is_none());
        subscription.select(Some("fiat_token_v2_2__transfer".into()));
        let selection = next(&snapshots).selection.clone().unwrap().unwrap();
        assert_eq!(selection.rows, Some(15_958));
        // Selecting what is already selected is not a reason to poll again.
        subscription.select(Some("fiat_token_v2_2__transfer".into()));
        assert!(snapshots.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn a_resubscriber_after_the_worker_left_gets_a_new_worker() {
        let nest = MockNest::recorded();
        let hub = hub();
        let (sink, snapshots) = channel();
        drop(hub.subscribe(&nest.url(), sink));
        drop(snapshots);
        settles_to_no_workers(&hub);

        let (sink, snapshots) = channel();
        let _subscription = hub.subscribe(&nest.url(), sink);
        next(&snapshots);
        assert_eq!(hub.workers(), 1);
    }

    #[test]
    fn a_nest_that_is_down_is_still_reported() {
        let nest = MockNest::start();
        nest.route("/ready", 503, fixtures::READY_503);
        let hub = hub();
        let (sink, snapshots) = channel();
        let _subscription = hub.subscribe(&nest.url(), sink);
        let snapshot = next(&snapshots);
        assert!(snapshot.ready.as_ref().unwrap().stalled);
        assert_eq!(
            snapshot.identity_problem,
            Some(("/tables", Error::Status(404)))
        );
    }
}
