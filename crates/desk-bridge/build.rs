use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("desk.bridge"))
        // Qt Qml needs Qt Network at link time on macOS.
        .qt_module("Network")
        .files([
            "src/nest_status.rs",
        ])
        .build()
        .export();
}
