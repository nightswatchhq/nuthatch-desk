//! The Qt base classes the models derive from, declared once.
//!
//! cxx-qt generates a pair of cast helpers for every `#[qobject]` it is told about, under a name
//! made from the type's. Declaring `QAbstractListModel` in each bridge that uses it therefore
//! defines those helpers twice, which the macOS linker lets pass and every Linux linker refuses.
//! So each base is declared here and the model bridges refer to it.

/// The bridge for the base classes.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        /// `QAbstractListModel`, the base of `TableList` and `MetricSeries`.
        #[qobject]
        type QAbstractListModel;

        include!(<QtCore/QAbstractTableModel>);
        /// `QAbstractTableModel`, the base of `ResultModel`.
        #[qobject]
        type QAbstractTableModel;
    }
}
