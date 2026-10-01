//! `TableList`: a nest's catalogue, and the table being previewed.

/// The bridge for [`TableList`](qobject::TableList).
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
    }

    extern "RustQt" {
        /// A nest's catalogue as a list model, and the table being previewed.
        ///
        /// - **Owner:** QML. The Rust struct lives inside the C++ object and drops with it.
        /// - **Thread:** the GUI thread, for every property, signal, invokable and model call.
        /// - **Crosses the boundary:** `QString`, integers and booleans, by value. The preview's
        ///   rows do not cross here: they are published under `feedKey` for a `ResultModel`.
        /// - **Worker:** none of its own. It subscribes to the nest's shared poller.
        ///
        /// Rows are `table`, `group`, `name` and `call`. The catalogue is fixed for the life of a
        /// Nuthatch process, so the model resets only when a restart brings a different one.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, url, READ, WRITE = set_url, NOTIFY = url_changed)]
        #[qproperty(i32, count, READ, NOTIFY = changed)]
        #[qproperty(i32, selected, READ, NOTIFY = changed)]
        #[qproperty(QString, selected_table, cxx_name = "selectedTable", READ, NOTIFY = changed)]
        #[qproperty(i64, selected_rows, cxx_name = "selectedRows", READ, NOTIFY = changed)]
        #[qproperty(i64, selected_latest_block, cxx_name = "selectedLatestBlock", READ, NOTIFY = changed)]
        #[qproperty(i64, feed_key, cxx_name = "feedKey", READ, NOTIFY = changed)]
        #[qproperty(QString, feed_notice, cxx_name = "feedNotice", READ, NOTIFY = changed)]
        #[qproperty(bool, sql_open, cxx_name = "sqlOpen", READ, NOTIFY = changed)]
        type TableList = super::TableListRust;

        /// `url` changed.
        #[qsignal]
        #[cxx_name = "urlChanged"]
        fn url_changed(self: Pin<&mut TableList>);

        /// Some read-only property changed.
        #[qsignal]
        fn changed(self: Pin<&mut TableList>);

        /// Lists the nest at `url`, in place of any listed before.
        #[cxx_name = "setUrl"]
        fn set_url(self: Pin<&mut TableList>, url: QString);

        /// Previews the table at `row` from now on.
        #[qinvokable]
        fn select(self: Pin<&mut TableList>, row: i32);
    }

    impl cxx_qt::Threading for TableList {}

    // The overrides Qt calls.
    extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &TableList, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        fn data(self: &TableList, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &TableList) -> QHash_i32_QByteArray;
    }

    // The reset bracket of the base class, used only by `TableList::replace`.
    extern "RustQt" {
        /// # Safety
        ///
        /// Must be followed by `end_reset_model`.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut TableList>);

        /// # Safety
        ///
        /// Must follow `begin_reset_model`.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut TableList>);
    }
}

use std::{pin::Pin, sync::Arc};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use desk_core::{
    catalogue::{self, Entry},
    feed::{Delivery, Hub, Sink, Subscription},
    grid::Grid,
    results,
};
use nest_client::{
    config::check_url,
    poll::Snapshot,
    types::{Identity, SqlAccess},
};

use crate::guard::{contained, contained_or, to_i32, to_i64, to_index};

/// `Qt::UserRole`, the first role a model may define.
const TABLE_ROLE: i32 = 0x0100;
const GROUP_ROLE: i32 = TABLE_ROLE + 1;
const NAME_ROLE: i32 = TABLE_ROLE + 2;
const CALL_ROLE: i32 = TABLE_ROLE + 3;

/// The Rust state behind [`qobject::TableList`].
pub struct TableListRust {
    url: QString,
    count: i32,
    selected: i32,
    selected_table: QString,
    selected_rows: i64,
    selected_latest_block: i64,
    feed_key: i64,
    feed_notice: QString,
    sql_open: bool,

    entries: Vec<Entry>,
    /// The catalogue the entries were made from, to tell a new one by address.
    identity: Option<Arc<Identity>>,
    /// The preview last published, to publish again only when it differs.
    feed: Option<Arc<Grid>>,
    subscription: Option<Subscription>,
    /// Counts changes of `url`, so a snapshot queued for an earlier one is not applied.
    generation: u64,
}

impl Default for TableListRust {
    fn default() -> Self {
        Self {
            url: QString::default(),
            count: 0,
            selected: -1,
            selected_table: QString::default(),
            selected_rows: -1,
            selected_latest_block: -1,
            feed_key: 0,
            feed_notice: QString::default(),
            sql_open: true,
            entries: Vec::new(),
            identity: None,
            feed: None,
            subscription: None,
            generation: 0,
        }
    }
}

impl Drop for TableListRust {
    fn drop(&mut self) {
        results::retire(self.feed_key);
    }
}

impl qobject::TableList {
    fn set_url(mut self: Pin<&mut Self>, url: QString) {
        contained("TableList.url", || {
            if url == self.url {
                return;
            }
            let generation = self.generation + 1;
            {
                let mut rust = self.as_mut().rust_mut();
                rust.generation = generation;
                rust.subscription = None;
                rust.identity = None;
                rust.url = url.clone();
            }
            self.as_mut().replace(Vec::new());
            self.as_mut().forget_preview();
            self.as_mut().url_changed();
            self.as_mut().changed();

            // A URL that cannot be polled lists nothing; `NestStatus` is where the reason shows.
            let (Ok(endpoint), Ok(hub)) = (check_url(&url.to_string()), Hub::global()) else {
                return;
            };
            let thread = self.qt_thread();
            let sink: Sink = Box::new(move |snapshot| {
                let snapshot = Arc::clone(snapshot);
                let queued = thread.queue(move |list| {
                    contained("TableList's update", || list.apply(generation, &snapshot));
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

    fn select(mut self: Pin<&mut Self>, row: i32) {
        contained("TableList.select", || {
            let Some(entry) = to_index(row).and_then(|row| self.entries.get(row)) else {
                return;
            };
            if row == self.selected {
                return;
            }
            let table = entry.table.clone();
            {
                let mut rust = self.as_mut().rust_mut();
                rust.selected = row;
                rust.selected_table = QString::from(&table);
            }
            self.as_mut().forget_preview();
            if let Some(subscription) = &self.subscription {
                subscription.select(Some(table));
            }
            self.changed();
        });
    }

    /// Drops what was known of the previewed table's rows. The model showing them keeps its own.
    fn forget_preview(mut self: Pin<&mut Self>) {
        let retired = self.feed_key;
        {
            let mut rust = self.as_mut().rust_mut();
            rust.selected_rows = -1;
            rust.selected_latest_block = -1;
            rust.feed_key = 0;
            rust.feed_notice = QString::default();
            rust.feed = None;
        }
        results::retire(retired);
    }

    /// Runs on the GUI thread, queued by the poller.
    fn apply(mut self: Pin<&mut Self>, generation: u64, snapshot: &Snapshot) {
        if generation != self.generation {
            return;
        }
        if let Some(identity) = &snapshot.identity {
            let known = self
                .identity
                .as_ref()
                .is_some_and(|known| Arc::ptr_eq(known, identity));
            if !known {
                self.as_mut().adopt(identity);
            }
        }
        if let Some(Ok(selection)) = &snapshot.selection
            && selection.table == self.selected_table.to_string()
        {
            let grid = Grid::from(selection.newest.clone());
            let notice = grid.notice();
            {
                let mut rust = self.as_mut().rust_mut();
                rust.selected_rows = selection.rows.map_or(-1, to_i64);
                rust.selected_latest_block = selection.latest_block.map_or(-1, to_i64);
                rust.feed_notice = QString::from(&notice);
            }
            if self.feed.as_deref() != Some(&grid) {
                let grid = Arc::new(grid);
                let retired = self.feed_key;
                {
                    let mut rust = self.as_mut().rust_mut();
                    rust.feed_key = results::publish(Grid::clone(&grid));
                    rust.feed = Some(grid);
                }
                results::retire(retired);
            }
        }
        self.changed();
    }

    /// Takes on a catalogue: the first, or a different one after a restart.
    fn adopt(mut self: Pin<&mut Self>, identity: &Arc<Identity>) {
        let entries = catalogue::entries(identity);
        let sql_open = identity.sql == SqlAccess::Open;
        {
            let mut rust = self.as_mut().rust_mut();
            rust.identity = Some(Arc::clone(identity));
            rust.sql_open = sql_open;
        }
        if entries == self.entries {
            return;
        }
        // The place is kept by name, so the same table stays selected across a restart.
        let wanted = self.selected_table.to_string();
        let selected = entries
            .iter()
            .position(|entry| entry.table == wanted)
            .or(if entries.is_empty() { None } else { Some(0) });
        let table = selected.map(|row| entries[row].table.clone());
        self.as_mut().replace(entries);
        let moved = table.as_deref() != Some(wanted.as_str());
        {
            let mut rust = self.as_mut().rust_mut();
            rust.selected = selected.map_or(-1, to_i32);
            rust.selected_table = QString::from(table.as_deref().unwrap_or_default());
        }
        if moved {
            self.as_mut().forget_preview();
        }
        if let Some(subscription) = &self.subscription {
            subscription.select(table);
        }
    }

    /// Replaces the rows, inside a reset bracket.
    fn replace(mut self: Pin<&mut Self>, entries: Vec<Entry>) {
        // SAFETY: the bracket is closed below with nothing between that can return or unwind past
        // it: the assignments move plain Rust values.
        unsafe { self.as_mut().begin_reset_model() };
        {
            let mut rust = self.as_mut().rust_mut();
            rust.count = to_i32(entries.len());
            rust.entries = entries;
            rust.selected = -1;
        }
        // SAFETY: closes the reset opened above.
        unsafe { self.as_mut().end_reset_model() };
    }

    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        to_i32(self.entries.len())
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        contained_or("TableList.data", QVariant::default(), || {
            let Some(entry) = to_index(index.row()).and_then(|row| self.entries.get(row)) else {
                return QVariant::default();
            };
            let text = |text: &str| QVariant::from(&QString::from(text));
            match role {
                TABLE_ROLE => text(&entry.table),
                GROUP_ROLE => text(&entry.group),
                NAME_ROLE => text(&entry.name),
                CALL_ROLE => QVariant::from(&entry.call),
                _ => QVariant::default(),
            }
        })
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        roles.insert(TABLE_ROLE, QByteArray::from("table"));
        roles.insert(GROUP_ROLE, QByteArray::from("group"));
        roles.insert(NAME_ROLE, QByteArray::from("name"));
        roles.insert(CALL_ROLE, QByteArray::from("call"));
        roles
    }
}
