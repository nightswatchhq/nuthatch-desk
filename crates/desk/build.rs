use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("desk.app").qml_files([
        "qml/Chart.qml",
        "qml/DataGrid.qml",
        "qml/Main.qml",
        "qml/Mono.qml",
        "qml/NestPage.qml",
        "qml/PlainLabel.qml",
        "qml/Shell.qml",
        "qml/Stat.qml",
        "qml/StateBadge.qml",
    ]))
    // Qt Qml needs Qt Network at link time on macOS.
    .qt_module("Network")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .build();
}
