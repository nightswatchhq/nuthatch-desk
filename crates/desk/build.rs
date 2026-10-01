use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("desk.app").qml_file("qml/Main.qml"))
        .qt_module("Network")
        .build();
}
