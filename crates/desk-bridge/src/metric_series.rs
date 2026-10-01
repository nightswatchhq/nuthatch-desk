//! `MetricSeries`: one metric's history.

/// The bridge for [`MetricSeries`](qobject::MetricSeries).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        /// The Qt base class.
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// `QString` from `cxx_qt_lib`.
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        /// `QVariant` from `cxx_qt_lib`.
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qmodelindex.h");
        /// `QModelIndex` from `cxx_qt_lib`.
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qhash.h");
        /// `QHash<int, QByteArray>` from `cxx_qt_lib`.
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qlist.h");
        /// `QList<QPointF>` from `cxx_qt_lib`.
        type QList_QPointF = cxx_qt_lib::QList<cxx_qt_lib::QPointF>;
    }

    extern "RustQt" {
        /// One metric's history as a list model, oldest point first.
        ///
        /// - **Owner:** QML. The Rust struct lives inside the C++ object and drops with it.
        /// - **Thread:** the GUI thread, for every property, signal, invokable and model call.
        /// - **Crosses the boundary:** `QString`, numbers, and a list of points by value.
        /// - **Worker:** none of its own. It subscribes to the nest's shared poller.
        ///
        /// `metric` is one of `lag`, `seal_gap`, `refresh_ms`, `cpu`, `rate:<family>` or
        /// `gauge:<family>`. Rows are `at`, in seconds since the series began, and `value`. A point
        /// is appended per poll and points older than `windowSecs` are removed from the front, each
        /// inside its own bracket. The history lives here, so it lasts as long as the object does.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, url, READ, WRITE = set_url, NOTIFY = url_changed)]
        #[qproperty(QString, metric, READ, WRITE = set_metric, NOTIFY = metric_changed)]
        #[qproperty(i32, window_secs, cxx_name = "windowSecs", READ, WRITE = set_window_secs, NOTIFY = window_secs_changed)]
        #[qproperty(i32, count, READ, NOTIFY = changed)]
        #[qproperty(f64, peak, READ, NOTIFY = changed)]
        #[qproperty(f64, latest, READ, NOTIFY = changed)]
        #[qproperty(bool, known, READ, NOTIFY = changed)]
        #[qproperty(i32, revision, READ, NOTIFY = changed)]
        type MetricSeries = super::MetricSeriesRust;

        /// `url` changed.
        #[qsignal]
        #[cxx_name = "urlChanged"]
        fn url_changed(self: Pin<&mut MetricSeries>);

        /// `metric` changed.
        #[qsignal]
        #[cxx_name = "metricChanged"]
        fn metric_changed(self: Pin<&mut MetricSeries>);

        /// `windowSecs` changed.
        #[qsignal]
        #[cxx_name = "windowSecsChanged"]
        fn window_secs_changed(self: Pin<&mut MetricSeries>);

        /// The points changed.
        #[qsignal]
        fn changed(self: Pin<&mut MetricSeries>);

        /// Follows the nest at `url`, in place of any followed before. The history is dropped.
        #[cxx_name = "setUrl"]
        fn set_url(self: Pin<&mut MetricSeries>, url: QString);

        /// Measures `metric` from now on. The history is dropped.
        #[cxx_name = "setMetric"]
        fn set_metric(self: Pin<&mut MetricSeries>, metric: QString);

        /// Keeps `seconds` of history from now on.
        #[cxx_name = "setWindowSecs"]
        fn set_window_secs(self: Pin<&mut MetricSeries>, seconds: i32);

        /// Drops the history.
        #[qinvokable]
        fn clear(self: Pin<&mut MetricSeries>);

        /// The line through the points in a box `width` by `height`: the newest point at the right
        /// edge, one window before it at the left, the peak at the top.
        #[qinvokable]
        fn polyline(self: &MetricSeries, width: f64, height: f64) -> QList_QPointF;
    }

    impl cxx_qt::Threading for MetricSeries {}

    // The overrides Qt calls.
    extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &MetricSeries, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        fn data(self: &MetricSeries, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &MetricSeries) -> QHash_i32_QByteArray;
    }

    // The brackets of the base class, used only by `MetricSeries::drop_front` and `append`.
    extern "RustQt" {
        /// # Safety
        ///
        /// Must be followed by `end_insert_rows`, with exactly rows `first..=last` added between.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut MetricSeries>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        /// # Safety
        ///
        /// Must follow `begin_insert_rows`.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut MetricSeries>);

        /// # Safety
        ///
        /// Must be followed by `end_remove_rows`, with exactly rows `first..=last` removed between.
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut MetricSeries>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        /// # Safety
        ///
        /// Must follow `begin_remove_rows`.
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut MetricSeries>);
    }
}

use std::{pin::Pin, sync::Arc, time::Duration};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QPointF, QString, QVariant,
};
use desk_core::{
    feed::{Delivery, Hub, Sink, Subscription},
    series::{Metric, Point, Series},
};
use nest_client::{config::check_url, poll::Snapshot};

use crate::guard::{contained, contained_or, to_i32, to_index};

/// `Qt::UserRole`, the first role a model may define.
const AT_ROLE: i32 = 0x0100;
const VALUE_ROLE: i32 = AT_ROLE + 1;
/// An hour, the history the RFC asks for.
const DEFAULT_WINDOW_SECS: i32 = 3600;

/// The Rust state behind [`qobject::MetricSeries`].
pub struct MetricSeriesRust {
    url: QString,
    metric: QString,
    window_secs: i32,
    count: i32,
    peak: f64,
    latest: f64,
    /// Whether `metric` names something this client can measure.
    known: bool,
    /// Goes up with every change to the points, for a binding that draws them to depend on.
    revision: i32,

    series: Series,
    subscription: Option<Subscription>,
    /// Counts changes of `url`, so a snapshot queued for an earlier one is not applied.
    generation: u64,
}

impl Default for MetricSeriesRust {
    fn default() -> Self {
        Self {
            url: QString::default(),
            metric: QString::default(),
            window_secs: DEFAULT_WINDOW_SECS,
            count: 0,
            peak: 0.0,
            latest: 0.0,
            known: false,
            revision: 0,
            series: Series::new(None, Duration::from_secs(DEFAULT_WINDOW_SECS as u64)),
            subscription: None,
            generation: 0,
        }
    }
}

impl qobject::MetricSeries {
    fn set_url(mut self: Pin<&mut Self>, url: QString) {
        contained("MetricSeries.url", || {
            if url == self.url {
                return;
            }
            let generation = self.generation + 1;
            {
                let mut rust = self.as_mut().rust_mut();
                rust.generation = generation;
                rust.subscription = None;
                rust.url = url.clone();
            }
            self.as_mut().empty();
            self.as_mut().url_changed();

            let (Ok(endpoint), Ok(hub)) = (check_url(&url.to_string()), Hub::global()) else {
                return;
            };
            let thread = self.qt_thread();
            let sink: Sink = Box::new(move |snapshot| {
                let snapshot = Arc::clone(snapshot);
                let queued = thread.queue(move |series| {
                    contained("MetricSeries's update", || series.apply(generation, &snapshot));
                });
                match queued {
                    Ok(()) => Delivery::Taken,
                    Err(_) => Delivery::Gone,
                }
            });
            let subscription = hub.subscribe(&endpoint.url, sink);
            self.as_mut().rust_mut().subscription = Some(subscription);
        });
    }

    fn set_metric(mut self: Pin<&mut Self>, metric: QString) {
        contained("MetricSeries.metric", || {
            if metric == self.metric {
                return;
            }
            let parsed = Metric::parse(&metric.to_string());
            let held = self.series.len();
            self.as_mut().drop_front(held);
            {
                let mut rust = self.as_mut().rust_mut();
                rust.known = parsed.is_some();
                rust.series.set_metric(parsed);
                rust.metric = metric;
            }
            self.as_mut().metric_changed();
            self.publish();
        });
    }

    fn set_window_secs(mut self: Pin<&mut Self>, seconds: i32) {
        contained("MetricSeries.windowSecs", || {
            let seconds = seconds.max(1);
            if seconds == self.window_secs {
                return;
            }
            let window = Duration::from_secs(u64::from(seconds.unsigned_abs()));
            // Counted first, so the points a shorter window sheds go inside a removal bracket.
            let stale = self.series.stale_under(window);
            self.as_mut().drop_front(stale);
            {
                let mut rust = self.as_mut().rust_mut();
                rust.series.set_window(window);
                rust.window_secs = seconds;
            }
            self.as_mut().window_secs_changed();
            self.publish();
        });
    }

    fn clear(mut self: Pin<&mut Self>) {
        contained("MetricSeries.clear", || self.as_mut().empty());
    }

    /// Drops every point and the counter reading behind a rate.
    fn empty(mut self: Pin<&mut Self>) {
        let held = self.series.len();
        self.as_mut().drop_front(held);
        self.as_mut().rust_mut().series.clear();
        self.publish();
    }

    fn polyline(&self, width: f64, height: f64) -> QList<QPointF> {
        contained_or("MetricSeries.polyline", QList::default(), || {
            let mut line = QList::default();
            for (x, y) in self.series.polyline(width, height) {
                line.append(QPointF::new(x, y));
            }
            line
        })
    }

    /// Runs on the GUI thread, queued by the poller.
    fn apply(mut self: Pin<&mut Self>, generation: u64, snapshot: &Snapshot) {
        if generation != self.generation {
            return;
        }
        let (change, point) = self.as_mut().rust_mut().series.plan(snapshot);
        self.as_mut().drop_front(change.dropped);
        if let Some(point) = point {
            self.as_mut().append(point);
        }
        if change.dropped > 0 || change.appended {
            self.publish();
        }
    }

    /// Removes `count` points from the front, inside a removal bracket.
    fn drop_front(mut self: Pin<&mut Self>, count: usize) {
        let count = count.min(self.series.len());
        if count == 0 {
            return;
        }
        // SAFETY: rows `0..count` are removed by the call below and the bracket is closed straight
        // after it. `drop_front` drains a `VecDeque` of `Copy` points and cannot unwind.
        unsafe {
            self.as_mut()
                .begin_remove_rows(&QModelIndex::default(), 0, to_i32(count - 1));
        }
        self.as_mut().rust_mut().series.drop_front(count);
        // SAFETY: closes the removal opened above.
        unsafe { self.as_mut().end_remove_rows() };
    }

    /// Appends one point, inside an insertion bracket.
    fn append(mut self: Pin<&mut Self>, point: Point) {
        let at = to_i32(self.series.len());
        // SAFETY: row `at` is added by the push below and the bracket is closed straight after it.
        // A failed allocation in the push aborts rather than unwinds.
        unsafe {
            self.as_mut()
                .begin_insert_rows(&QModelIndex::default(), at, at);
        }
        self.as_mut().rust_mut().series.push(point);
        // SAFETY: closes the insertion opened above.
        unsafe { self.as_mut().end_insert_rows() };
    }

    /// Copies the series' figures into the properties and says they changed.
    fn publish(mut self: Pin<&mut Self>) {
        {
            let (count, peak, latest) = (
                to_i32(self.series.len()),
                self.series.peak(),
                self.series.latest().unwrap_or_default(),
            );
            let mut rust = self.as_mut().rust_mut();
            rust.count = count;
            rust.peak = peak;
            rust.latest = latest;
            rust.revision = rust.revision.wrapping_add(1);
        }
        self.changed();
    }

    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        to_i32(self.series.len())
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        contained_or("MetricSeries.data", QVariant::default(), || {
            let Some(point) = to_index(index.row()).and_then(|row| self.series.point(row)) else {
                return QVariant::default();
            };
            match role {
                AT_ROLE => QVariant::from(&point.at),
                VALUE_ROLE => QVariant::from(&point.value),
                _ => QVariant::default(),
            }
        })
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        roles.insert(AT_ROLE, QByteArray::from("at"));
        roles.insert(VALUE_ROLE, QByteArray::from("value"));
        roles
    }
}
