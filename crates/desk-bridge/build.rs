use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    // The dependency tells qmllint where the models' base classes are declared.
    CxxQtBuilder::new_qml_module(QmlModule::new("desk.bridge").depend("QtQml.Models"))
        // Qt Qml needs Qt Network at link time on macOS.
        .qt_module("Network")
        .files([
            "src/bases.rs",
            "src/metric_series.rs",
            "src/nest_status.rs",
            "src/result_model.rs",
            "src/sql_query.rs",
            "src/table_list.rs",
        ])
        .build()
        .export();
}
