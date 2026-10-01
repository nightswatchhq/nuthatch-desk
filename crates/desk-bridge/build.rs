use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("desk.bridge"))
        // Qt Qml needs Qt Network at link time on macOS.
        .qt_module("Network")
        .files([
            "src/metric_series.rs",
            "src/nest_status.rs",
            "src/result_model.rs",
            "src/sql_query.rs",
            "src/table_list.rs",
        ])
        .build()
        .export();
}
