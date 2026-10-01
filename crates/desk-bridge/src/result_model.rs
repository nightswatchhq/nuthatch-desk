//! `ResultModel`: a result as a table model, sortable.

/// The bridge for [`ResultModel`](qobject::ResultModel).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractTableModel>);
        /// The Qt base class.
        #[qobject]
        type QAbstractTableModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// `QString` from `cxx_qt_lib`.
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        /// `QStringList` from `cxx_qt_lib`.
        type QStringList = cxx_qt_lib::QStringList;
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

    #[namespace = "Qt"]
    unsafe extern "C++" {
        include!("cxx-qt-lib/qt.h");
        /// `Qt::Orientation` from `cxx_qt_lib`.
        type Orientation = cxx_qt_lib::Orientation;
    }

    extern "RustQt" {
        /// A result as a table model.
        ///
        /// - **Owner:** QML. The Rust struct lives inside the C++ object and drops with it.
        /// - **Thread:** the GUI thread only. It has no worker and takes no queued calls.
        /// - **Crosses the boundary:** every cell as a `QString`, never as a number. A token amount
        ///   is 256 bits and a QML number is a double.
        ///
        /// It shows whatever grid is published under `resultKey`, which QML binds to the object
        /// that fetched it (`SqlQuery.resultKey`, `TableList.feedKey`). When the key changes the
        /// rows change inside the narrowest bracket that is true: an insert at the top and a
        /// removal at the bottom for a feed that has moved on, a reset for anything else.
        #[qobject]
        #[qml_element]
        #[base = QAbstractTableModel]
        #[qproperty(i64, result_key, cxx_name = "resultKey", READ, WRITE = set_result_key, NOTIFY = result_key_changed)]
        #[qproperty(QStringList, column_names, cxx_name = "columnNames", READ, NOTIFY = shape_changed)]
        #[qproperty(i32, rows, READ, NOTIFY = shape_changed)]
        #[qproperty(i32, sort_column, cxx_name = "sortColumn", READ, NOTIFY = sort_changed)]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending", READ, NOTIFY = sort_changed)]
        type ResultModel = super::ResultModelRust;

        /// `resultKey` changed.
        #[qsignal]
        #[cxx_name = "resultKeyChanged"]
        fn result_key_changed(self: Pin<&mut ResultModel>);

        /// The columns or the row count changed.
        #[qsignal]
        #[cxx_name = "shapeChanged"]
        fn shape_changed(self: Pin<&mut ResultModel>);

        /// The sort column or direction changed.
        #[qsignal]
        #[cxx_name = "sortChanged"]
        fn sort_changed(self: Pin<&mut ResultModel>);

        /// Shows the grid published under `key`, or nothing if there is none.
        #[cxx_name = "setResultKey"]
        fn set_result_key(self: Pin<&mut ResultModel>, key: i64);

        /// Sorts by `column`, ascending, or flips the direction if it is already the sort column.
        #[qinvokable]
        #[cxx_name = "sortBy"]
        fn sort_by(self: Pin<&mut ResultModel>, column: i32);

        /// Goes back to the order the rows arrived in.
        #[qinvokable]
        #[cxx_name = "clearSort"]
        fn clear_sort(self: Pin<&mut ResultModel>);

        /// The rows as CSV, in the order shown, with a header line.
        #[qinvokable]
        fn csv(self: &ResultModel) -> QString;
    }

    // The overrides Qt calls.
    extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &ResultModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "columnCount"]
        fn column_count(self: &ResultModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        fn data(self: &ResultModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "headerData"]
        fn header_data(
            self: &ResultModel,
            section: i32,
            orientation: Orientation,
            role: i32,
        ) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &ResultModel) -> QHash_i32_QByteArray;
    }

    // The brackets of the base class. Each `begin` must be followed by its `end` with the rows
    // changed in between and nowhere else, which `ResultModel`'s three private helpers see to.
    extern "RustQt" {
        /// # Safety
        ///
        /// Must be followed by `end_reset_model`.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut ResultModel>);

        /// # Safety
        ///
        /// Must follow `begin_reset_model`.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut ResultModel>);

        /// # Safety
        ///
        /// Must be followed by `end_insert_rows`, with exactly rows `first..=last` added between.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut ResultModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        /// # Safety
        ///
        /// Must follow `begin_insert_rows`.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut ResultModel>);

        /// # Safety
        ///
        /// Must be followed by `end_remove_rows`, with exactly rows `first..=last` removed between.
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut ResultModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        /// # Safety
        ///
        /// Must follow `begin_remove_rows`.
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut ResultModel>);
    }
}

use std::{pin::Pin, sync::Arc};

use cxx_qt::CxxQtType;
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QStringList, QVariant,
};
use desk_core::{
    grid::{Grid, Patch},
    results,
};

use crate::guard::{contained, contained_or, to_i32, to_index};

/// `Qt::DisplayRole`.
const DISPLAY_ROLE: i32 = 0;
/// `Qt::UserRole + 1`: whether the cell is SQL `NULL` rather than an empty string.
const IS_NULL_ROLE: i32 = 0x0101;

/// The Rust state behind [`qobject::ResultModel`].
pub struct ResultModelRust {
    result_key: i64,
    column_names: QStringList,
    rows: i32,
    sort_column: i32,
    sort_descending: bool,

    grid: Arc<Grid>,
    /// The grid's row for each row shown. The identity until sorted.
    order: Vec<usize>,
}

impl Default for ResultModelRust {
    fn default() -> Self {
        Self {
            result_key: 0,
            column_names: QStringList::default(),
            rows: 0,
            sort_column: -1,
            sort_descending: false,
            grid: Arc::default(),
            order: Vec::new(),
        }
    }
}

impl ResultModelRust {
    /// The order to show `grid` in under the current sort.
    fn order_of(&self, grid: &Grid) -> Vec<usize> {
        match to_index(self.sort_column) {
            Some(column) => grid.sorted(column, self.sort_descending),
            None => (0..grid.rows.len()).collect(),
        }
    }
}

impl qobject::ResultModel {
    fn set_result_key(mut self: Pin<&mut Self>, key: i64) {
        contained("ResultModel.resultKey", || {
            if key == self.result_key {
                return;
            }
            self.as_mut().rust_mut().result_key = key;
            let new = results::get(key).unwrap_or_default();
            let patch = if self.sort_column < 0 {
                self.grid.patch(&new)
            } else {
                // Sorted rows do not slide: a new row belongs wherever its value puts it.
                Patch::Reset
            };
            match patch {
                Patch::Same => self.as_mut().rust_mut().grid = new,
                Patch::Slide { fresh, dropped } => self.as_mut().slide(new, fresh, dropped),
                Patch::Reset => {
                    let order = self.order_of(&new);
                    self.as_mut().reset(new, order);
                }
            }
            self.result_key_changed();
        });
    }

    fn sort_by(mut self: Pin<&mut Self>, column: i32) {
        contained("ResultModel.sortBy", || {
            if to_index(column).is_none_or(|column| column >= self.grid.columns.len()) {
                return;
            }
            let descending = column == self.sort_column && !self.sort_descending;
            {
                let mut rust = self.as_mut().rust_mut();
                rust.sort_column = column;
                rust.sort_descending = descending;
            }
            self.as_mut().resort();
        });
    }

    fn clear_sort(mut self: Pin<&mut Self>) {
        contained("ResultModel.clearSort", || {
            if self.sort_column < 0 {
                return;
            }
            {
                let mut rust = self.as_mut().rust_mut();
                rust.sort_column = -1;
                rust.sort_descending = false;
            }
            self.as_mut().resort();
        });
    }

    fn resort(mut self: Pin<&mut Self>) {
        let grid = Arc::clone(&self.grid);
        let order = self.order_of(&grid);
        self.as_mut().reset(grid, order);
        self.sort_changed();
    }

    fn csv(&self) -> QString {
        contained_or("ResultModel.csv", QString::default(), || {
            QString::from(&self.grid.csv(&self.order))
        })
    }

    /// Replaces everything shown, inside a reset bracket.
    fn reset(mut self: Pin<&mut Self>, grid: Arc<Grid>, order: Vec<usize>) {
        let column_names: QStringList = grid.columns.iter().map(QString::from).collect();
        // SAFETY: the bracket is closed four lines down with nothing between that can return or
        // unwind past it: the assignments move plain Rust values.
        unsafe { self.as_mut().begin_reset_model() };
        {
            let mut rust = self.as_mut().rust_mut();
            rust.rows = to_i32(order.len());
            rust.column_names = column_names;
            rust.grid = grid;
            rust.order = order;
        }
        // SAFETY: closes the reset opened above.
        unsafe { self.as_mut().end_reset_model() };
        self.shape_changed();
    }

    /// Turns the unsorted grid shown into `new`, which is the same grid with `fresh` rows put on
    /// the front and `dropped` taken off the end.
    fn slide(mut self: Pin<&mut Self>, new: Arc<Grid>, fresh: usize, dropped: usize) {
        let root = QModelIndex::default();
        let old_len = self.order.len();
        if dropped > 0 && dropped <= old_len {
            let kept = old_len - dropped;
            // SAFETY: rows `kept..old_len` are removed by the truncation below and the bracket is
            // closed straight after it. `Vec::truncate` on a `Vec<usize>` cannot unwind.
            unsafe {
                self.as_mut()
                    .begin_remove_rows(&root, to_i32(kept), to_i32(old_len - 1));
            }
            {
                let mut rust = self.as_mut().rust_mut();
                rust.order.truncate(kept);
                rust.rows = to_i32(kept);
            }
            // SAFETY: closes the removal opened above.
            unsafe { self.as_mut().end_remove_rows() };
        }
        let order: Vec<usize> = (0..new.rows.len()).collect();
        if fresh > 0 {
            // SAFETY: rows `0..fresh` are added by swapping in the new grid, whose rows from
            // `fresh` on are the rows already shown, and the bracket is closed straight after.
            unsafe { self.as_mut().begin_insert_rows(&root, 0, to_i32(fresh - 1)) };
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.rows = to_i32(order.len());
            rust.grid = new;
            rust.order = order;
        }
        if fresh > 0 {
            // SAFETY: closes the insertion opened above.
            unsafe { self.as_mut().end_insert_rows() };
        }
        self.shape_changed();
    }

    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        to_i32(self.order.len())
    }

    fn column_count(&self, _parent: &QModelIndex) -> i32 {
        to_i32(self.grid.columns.len())
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        contained_or("ResultModel.data", QVariant::default(), || {
            let cell = to_index(index.row())
                .and_then(|row| self.order.get(row).copied())
                .zip(to_index(index.column()));
            let Some((row, column)) = cell else {
                return QVariant::default();
            };
            match role {
                DISPLAY_ROLE => self
                    .grid
                    .text(row, column)
                    .map_or_else(QVariant::default, |text| {
                        QVariant::from(&QString::from(&*text))
                    }),
                IS_NULL_ROLE => QVariant::from(&self.grid.is_null(row, column)),
                _ => QVariant::default(),
            }
        })
    }

    fn header_data(&self, section: i32, orientation: qobject::Orientation, role: i32) -> QVariant {
        contained_or("ResultModel.headerData", QVariant::default(), || {
            if role != DISPLAY_ROLE {
                return QVariant::default();
            }
            if orientation == qobject::Orientation::Horizontal {
                to_index(section)
                    .and_then(|column| self.grid.columns.get(column))
                    .map_or_else(QVariant::default, |name| {
                        QVariant::from(&QString::from(name))
                    })
            } else {
                QVariant::from(&(section + 1))
            }
        })
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        roles.insert(DISPLAY_ROLE, QByteArray::from("display"));
        roles.insert(IS_NULL_ROLE, QByteArray::from("isNull"));
        roles
    }
}
