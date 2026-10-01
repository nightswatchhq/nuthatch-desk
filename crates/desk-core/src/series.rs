//! One metric's history: a bounded run of points, and the line a chart draws through them.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use nest_client::poll::Snapshot;

/// Points kept however long the window: an hour at the fastest poll is 1,800.
const MAX_POINTS: usize = 4_096;

/// What a series measures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Metric {
    /// Blocks behind the tip, from `/ready`.
    Lag,
    /// Blocks indexed and not yet sealed, from `/ready`.
    SealGap,
    /// How long the client's own poll took, in milliseconds.
    RefreshMs,
    /// CPU use in percent of one core, from the process's CPU seconds.
    Cpu,
    /// A Prometheus counter, per second.
    Rate(String),
    /// A Prometheus gauge, as published.
    Gauge(String),
}

impl Metric {
    /// Reads a metric's name: `lag`, `seal_gap`, `refresh_ms`, `cpu`, `rate:<family>` or
    /// `gauge:<family>`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "lag" => Some(Self::Lag),
            "seal_gap" => Some(Self::SealGap),
            "refresh_ms" => Some(Self::RefreshMs),
            "cpu" => Some(Self::Cpu),
            _ => {
                let (kind, family) = name.split_once(':')?;
                if family.is_empty() {
                    return None;
                }
                match kind {
                    "rate" => Some(Self::Rate(family.to_owned())),
                    "gauge" => Some(Self::Gauge(family.to_owned())),
                    _ => None,
                }
            }
        }
    }

    /// The Prometheus counter this metric is the rate of, with the factor that scales it.
    fn counter(&self) -> Option<(&str, f64)> {
        match self {
            Self::Cpu => Some(("nuthatch_process_cpu_seconds_total", 100.0)),
            Self::Rate(family) => Some((family, 1.0)),
            _ => None,
        }
    }
}

/// One sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// When, in seconds since the series began.
    pub at: f64,
    /// The value.
    pub value: f64,
}

/// What recording one poll did to the series, in the terms a list model needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change {
    /// Points removed from the front, before any append.
    pub dropped: usize,
    /// Whether a point was appended at the back.
    pub appended: bool,
}

/// One metric's history over a window.
#[derive(Debug)]
pub struct Series {
    metric: Option<Metric>,
    window: Duration,
    origin: Instant,
    points: VecDeque<Point>,
    /// The counter's last reading, for a rate.
    before: Option<(Instant, f64)>,
}

impl Series {
    /// An empty series keeping `window` of history.
    pub fn new(metric: Option<Metric>, window: Duration) -> Self {
        Self {
            metric,
            window,
            origin: Instant::now(),
            points: VecDeque::new(),
            before: None,
        }
    }

    /// The metric measured.
    pub fn metric(&self) -> Option<&Metric> {
        self.metric.as_ref()
    }

    /// Measures something else from now on. The history is dropped; returns how many points went.
    pub fn set_metric(&mut self, metric: Option<Metric>) -> usize {
        self.metric = metric;
        self.clear()
    }

    /// How many points at the front would be older than `window`, were it the window.
    pub fn stale_under(&self, window: Duration) -> usize {
        let Some(newest) = self.points.back().map(|point| point.at) else {
            return 0;
        };
        let oldest_kept = newest - window.as_secs_f64();
        self.points
            .iter()
            .take_while(|point| point.at < oldest_kept)
            .count()
    }

    /// Keeps `window` of history from now on. Returns how many points fell out of the front.
    pub fn set_window(&mut self, window: Duration) -> usize {
        self.window = window;
        self.evict()
    }

    /// Drops the history. Returns how many points went.
    pub fn clear(&mut self) -> usize {
        self.before = None;
        let dropped = self.points.len();
        self.points.clear();
        dropped
    }

    /// The value this poll gives the metric, if it gives one.
    fn sample(&mut self, snapshot: &Snapshot) -> Option<f64> {
        let metric = self.metric.as_ref()?;
        if let Some((family, scale)) = metric.counter() {
            let reading = *snapshot.metrics.as_ref().ok()?.get(family)?;
            let before = self.before.replace((snapshot.taken, reading));
            // The first reading has nothing to be a rate against, and neither has the first after
            // a restart, when the counter began again from nought.
            let (then, was) = before.filter(|_| !snapshot.restarted)?;
            let seconds = snapshot.taken.saturating_duration_since(then).as_secs_f64();
            return (seconds > 0.0 && reading >= was).then(|| (reading - was) / seconds * scale);
        }
        match metric {
            Metric::Lag => snapshot.ready.as_ref().ok()?.lag_blocks.map(|lag| lag as f64),
            Metric::SealGap => {
                let ready = snapshot.ready.as_ref().ok()?;
                Some(ready.last_block.saturating_sub(ready.sealed_through) as f64)
            }
            Metric::RefreshMs => Some(snapshot.elapsed.as_secs_f64() * 1000.0),
            Metric::Gauge(family) => snapshot.metrics.as_ref().ok()?.get(family).copied(),
            Metric::Cpu | Metric::Rate(_) => None,
        }
    }

    fn evict(&mut self) -> usize {
        let Some(newest) = self.points.back().map(|point| point.at) else {
            return 0;
        };
        let oldest_kept = newest - self.window.as_secs_f64();
        let mut dropped = 0;
        while self.points.len() > MAX_POINTS
            || self.points.front().is_some_and(|point| point.at < oldest_kept)
        {
            self.points.pop_front();
            dropped += 1;
        }
        dropped
    }

    /// What recording `snapshot` will do to the points, without touching them yet.
    ///
    /// A list model has to announce a removal before it happens and an insertion before it happens,
    /// so the change is computed first and applied in two steps: [`Series::drop_front`] inside the
    /// removal bracket, [`Series::push`] inside the insertion bracket. Call once per poll: a rate
    /// takes its counter reading here.
    pub fn plan(&mut self, snapshot: &Snapshot) -> (Change, Option<Point>) {
        let Some(value) = self.sample(snapshot).filter(|value| value.is_finite()) else {
            return (Change::default(), None);
        };
        let point = Point {
            at: snapshot.taken.saturating_duration_since(self.origin).as_secs_f64(),
            value,
        };
        let oldest_kept = point.at - self.window.as_secs_f64();
        let stale = self
            .points
            .iter()
            .take_while(|point| point.at < oldest_kept)
            .count();
        let over = (self.points.len() + 1 - stale).saturating_sub(MAX_POINTS);
        (
            Change {
                dropped: stale + over,
                appended: true,
            },
            Some(point),
        )
    }

    /// Removes `count` points from the front.
    pub fn drop_front(&mut self, count: usize) {
        self.points.drain(..count.min(self.points.len()));
    }

    /// Appends a point.
    pub fn push(&mut self, point: Point) {
        self.points.push_back(point);
    }

    /// Records one poll in one step, for callers with no model to keep informed.
    pub fn record(&mut self, snapshot: &Snapshot) -> Change {
        let (change, point) = self.plan(snapshot);
        self.drop_front(change.dropped);
        if let Some(point) = point {
            self.push(point);
        }
        change
    }

    /// Points held.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether no point is held.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The point at `index`, oldest first.
    pub fn point(&self, index: usize) -> Option<Point> {
        self.points.get(index).copied()
    }

    /// The newest value.
    pub fn latest(&self) -> Option<f64> {
        self.points.back().map(|point| point.value)
    }

    /// The largest value held, or nought.
    pub fn peak(&self) -> f64 {
        self.points
            .iter()
            .map(|point| point.value)
            .fold(0.0, f64::max)
    }

    /// The line through the points, in a box `width` by `height` with the origin at the top left.
    ///
    /// The right edge is the newest point and the left edge is one window before it. The top is the
    /// peak. A series with fewer than two points has no line.
    pub fn polyline(&self, width: f64, height: f64) -> Vec<(f64, f64)> {
        let Some(newest) = self.points.back().map(|point| point.at) else {
            return Vec::new();
        };
        if self.points.len() < 2 || width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let window = self.window.as_secs_f64().max(f64::MIN_POSITIVE);
        let peak = self.peak();
        self.points
            .iter()
            .map(|point| {
                let x = width * (1.0 - (newest - point.at) / window);
                let fill = if peak > 0.0 { point.value / peak } else { 0.0 };
                (x.clamp(0.0, width), height * (1.0 - fill.clamp(0.0, 1.0)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nest_client::Error;

    use super::*;
    use crate::status::tests::{ready, snapshot};

    const RPC: &str = "nuthatch_rpc_requests_total";

    fn with_metric(at: Instant, family: &str, value: f64) -> Snapshot {
        let mut poll = snapshot(at, Ok(ready(1_000, 990)));
        poll.metrics = Ok(BTreeMap::from([(family.to_owned(), value)]));
        poll
    }

    fn series(metric: &str, window_secs: u64) -> Series {
        Series::new(Metric::parse(metric), Duration::from_secs(window_secs))
    }

    fn values(series: &Series) -> Vec<f64> {
        (0..series.len())
            .filter_map(|index| series.point(index))
            .map(|point| point.value)
            .collect()
    }

    #[test]
    fn metric_names_are_read_strictly() {
        assert_eq!(Metric::parse("lag"), Some(Metric::Lag));
        assert_eq!(Metric::parse("cpu"), Some(Metric::Cpu));
        assert_eq!(Metric::parse("rate:a_total"), Some(Metric::Rate("a_total".into())));
        assert_eq!(Metric::parse("gauge:nuthatch_rss_bytes"), Some(Metric::Gauge("nuthatch_rss_bytes".into())));
        for unknown in ["", "rate:", "sum:a", "Lag", "lag "] {
            assert_eq!(Metric::parse(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn a_gauge_from_ready_is_recorded_each_poll() {
        let start = Instant::now();
        let mut lag = series("lag", 3600);
        for (second, behind) in [(0, 10), (12, 4), (24, 0)] {
            let at = start + Duration::from_secs(second);
            lag.record(&snapshot(at, Ok(ready(1_000, 1_000 - behind))));
        }
        assert_eq!(values(&lag), [10.0, 4.0, 0.0]);
        assert_eq!(lag.peak(), 10.0);
        assert_eq!(lag.latest(), Some(0.0));
    }

    #[test]
    fn a_counter_becomes_a_rate_from_its_second_reading() {
        let start = Instant::now();
        let mut rate = series("rate:nuthatch_rpc_requests_total", 3600);
        assert_eq!(rate.record(&with_metric(start, RPC, 100.0)), Change::default());
        let change = rate.record(&with_metric(start + Duration::from_secs(10), RPC, 150.0));
        assert_eq!(change, Change { dropped: 0, appended: true });
        rate.record(&with_metric(start + Duration::from_secs(20), RPC, 150.0));
        assert_eq!(values(&rate), [5.0, 0.0]);
    }

    #[test]
    fn a_restart_gives_no_rate_and_no_negative_one() {
        let start = Instant::now();
        let mut rate = series("rate:nuthatch_rpc_requests_total", 3600);
        rate.record(&with_metric(start, RPC, 100.0));
        rate.record(&with_metric(start + Duration::from_secs(10), RPC, 150.0));
        let mut restarted = with_metric(start + Duration::from_secs(20), RPC, 3.0);
        restarted.restarted = true;
        assert!(!rate.record(&restarted).appended);
        // A counter that fell without the poller calling it a restart is still not a rate.
        assert!(!rate.record(&with_metric(start + Duration::from_secs(30), RPC, 1.0)).appended);
        rate.record(&with_metric(start + Duration::from_secs(40), RPC, 21.0));
        assert_eq!(values(&rate), [5.0, 2.0]);
    }

    #[test]
    fn cpu_is_percent_of_one_core() {
        let start = Instant::now();
        let family = "nuthatch_process_cpu_seconds_total";
        let mut cpu = series("cpu", 3600);
        cpu.record(&with_metric(start, family, 10.0));
        cpu.record(&with_metric(start + Duration::from_secs(4), family, 11.0));
        assert_eq!(values(&cpu), [25.0]);
    }

    #[test]
    fn a_poll_that_did_not_reach_the_nest_adds_nothing() {
        let start = Instant::now();
        let mut lag = series("lag", 3600);
        let mut gauge = series("gauge:nuthatch_rss_bytes", 3600);
        let mut down = snapshot(start, Err(Error::Connect));
        down.metrics = Err(Error::Connect);
        assert!(!lag.record(&down).appended);
        assert!(!gauge.record(&down).appended);
        // The client's own round trip is still a measurement.
        assert!(series("refresh_ms", 3600).record(&down).appended);
    }

    #[test]
    fn an_unknown_metric_records_nothing() {
        let mut none = Series::new(None, Duration::from_secs(60));
        assert!(!none.record(&snapshot(Instant::now(), Ok(ready(5, 5)))).appended);
        assert!(none.is_empty());
        assert!(none.polyline(100.0, 100.0).is_empty());
    }

    #[test]
    fn points_older_than_the_window_fall_out_of_the_front() {
        let start = Instant::now();
        let mut lag = series("lag", 60);
        for second in [0, 20, 40, 60] {
            lag.record(&snapshot(start + Duration::from_secs(second), Ok(ready(100, 100 - second / 20))));
        }
        assert_eq!(lag.len(), 4);
        let change = lag.record(&snapshot(start + Duration::from_secs(90), Ok(ready(100, 91))));
        assert_eq!(change, Change { dropped: 2, appended: true });
        assert_eq!(values(&lag), [2.0, 3.0, 9.0]);
        assert_eq!(lag.set_window(Duration::from_secs(10)), 2);
        assert_eq!(values(&lag), [9.0]);
    }

    #[test]
    fn the_plan_matches_what_recording_does() {
        let start = Instant::now();
        let mut planned = series("lag", 60);
        let mut recorded = series("lag", 60);
        recorded.origin = planned.origin;
        for second in (0..600).step_by(7) {
            let poll = snapshot(start + Duration::from_secs(second), Ok(ready(1_000, 1_000 - second % 13)));
            let before = planned.len();
            let (change, point) = planned.plan(&poll);
            planned.drop_front(change.dropped);
            assert_eq!(planned.len(), before - change.dropped);
            planned.push(point.unwrap());
            assert_eq!(recorded.record(&poll), change);
            assert_eq!(values(&planned), values(&recorded));
            // Nothing older than the window survives a step.
            let newest = planned.point(planned.len() - 1).unwrap().at;
            assert!(planned.point(0).unwrap().at >= newest - 60.0);
        }
    }

    #[test]
    fn an_hour_of_history_stays_one_size_however_long_it_runs() {
        let start = Instant::now();
        let mut lag = series("lag", 3600);
        let poll_every = 2;
        let mut record = |second: u64| {
            lag.record(&snapshot(start + Duration::from_secs(second), Ok(ready(second + 5, second))));
            (lag.len(), lag.points.capacity())
        };
        let mut after_one_hour = (0, 0);
        for second in (0..=3600).step_by(poll_every) {
            after_one_hour = record(second);
        }
        assert_eq!(after_one_hour.0, 1801);
        let mut after_six_hours = (0, 0);
        for second in (3602..=6 * 3600).step_by(poll_every) {
            after_six_hours = record(second);
        }
        assert_eq!(after_six_hours, after_one_hour);
    }

    #[test]
    fn the_point_count_is_capped_whatever_the_window() {
        let start = Instant::now();
        let mut lag = series("lag", 10_000_000);
        for second in 0..(MAX_POINTS as u64 + 500) {
            lag.record(&snapshot(start + Duration::from_secs(second), Ok(ready(9, 9))));
        }
        assert_eq!(lag.len(), MAX_POINTS);
    }

    #[test]
    fn the_line_spans_the_window_with_the_peak_at_the_top() {
        let start = Instant::now();
        let mut lag = series("lag", 100);
        for (second, behind) in [(0, 0), (50, 20), (100, 10)] {
            lag.record(&snapshot(start + Duration::from_secs(second), Ok(ready(1_000, 1_000 - behind))));
        }
        let line = lag.polyline(200.0, 80.0);
        assert_eq!(line.len(), 3);
        let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        assert!(close(line[0], (0.0, 80.0)), "{line:?}");
        assert!(close(line[1], (100.0, 0.0)), "{line:?}");
        assert!(close(line[2], (200.0, 40.0)), "{line:?}");
    }

    #[test]
    fn a_flat_zero_series_draws_along_the_bottom() {
        let start = Instant::now();
        let mut lag = series("lag", 100);
        lag.record(&snapshot(start, Ok(ready(7, 7))));
        assert!(lag.polyline(200.0, 80.0).is_empty());
        lag.record(&snapshot(start + Duration::from_secs(10), Ok(ready(7, 7))));
        assert!(lag.polyline(200.0, 80.0).iter().all(|&(_, y)| y == 80.0));
        assert!(lag.polyline(0.0, 80.0).is_empty());
    }

    #[test]
    fn clearing_forgets_the_counter_too() {
        let start = Instant::now();
        let mut rate = series("rate:nuthatch_rpc_requests_total", 3600);
        rate.record(&with_metric(start, RPC, 100.0));
        rate.record(&with_metric(start + Duration::from_secs(10), RPC, 150.0));
        assert_eq!(rate.clear(), 1);
        assert!(!rate.record(&with_metric(start + Duration::from_secs(20), RPC, 170.0)).appended);
    }
}
