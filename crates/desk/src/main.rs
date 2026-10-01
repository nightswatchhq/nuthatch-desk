//! nuthatch-desk: a read-only desktop client for a running Nuthatch nest.
//!
//! This file starts Qt and hands the QML its tabs. Everything the window shows is in
//! `desk-bridge`, and everything that decides it is in `desk-core`.

// On macOS the linker remarks that both cxx-qt build scripts gave it Qt's rpath, and that
// Homebrew's Qt was built for a newer macOS than the one in `.cargo/config.toml`.
#![allow(linker_messages)]

// Nothing here names a Rust item from the bridge, and without a reference its objects are not linked.
extern crate desk_bridge;

use std::process::ExitCode;

use cxx_qt_lib::{
    QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QQuickStyle, QString,
    QStringList, QUrl, QVariant,
};
use desk_core::startup::{Invocation, USAGE, parse_args, plan};
use nest_client::config::config_path;

fn main() -> ExitCode {
    let urls = match parse_args(std::env::args().skip(1)) {
        Ok(Invocation::Run(urls)) => urls,
        Ok(Invocation::Version) => {
            println!("nuthatch-desk {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Invocation::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(problem) => {
            eprintln!("{problem}");
            return ExitCode::from(2);
        }
    };
    // A file that is not there is not a problem; one that cannot be read is.
    let config = config_path()
        .filter(|path| path.exists())
        .map(|path| std::fs::read_to_string(&path).map_err(|error| error.to_string()));
    let (text, unreadable) = match config {
        Some(Ok(text)) => (Some(text), None),
        Some(Err(error)) => (None, Some(format!("nests.toml was not read: {error}"))),
        None => (None, None),
    };
    let plan = plan(&urls, text.as_deref());

    cxx_qt::init_crate!(desk_bridge);
    let mut app = QGuiApplication::new();
    // The platform styles refuse the custom tab titles, and one style reads the same everywhere.
    QQuickStyle::set_style(&QString::from("Fusion"));
    let mut engine = QQmlApplicationEngine::new();

    if let Some(mut engine) = engine.as_mut() {
        let list = |pick: fn(&desk_core::startup::Tab) -> &str| {
            let list: QStringList = plan
                .tabs
                .iter()
                .map(|tab| QString::from(pick(tab)))
                .collect();
            QVariant::from(&list)
        };
        let mut properties = QMap::<QMapPair_QString_QVariant>::default();
        properties.insert(QString::from("nestNames"), list(|tab| &tab.name));
        properties.insert(QString::from("nestUrls"), list(|tab| &tab.url));
        properties.insert(QString::from("nestNotes"), list(|tab| &tab.note));
        properties.insert(
            QString::from("configProblem"),
            QVariant::from(&QString::from(&unreadable.unwrap_or(plan.problem))),
        );
        engine.as_mut().set_initial_properties(&properties);
        // Without a window there is nothing to close, and the process would sit there for ever.
        engine
            .as_mut()
            .on_object_creation_failed(|_, _| {
                eprintln!("nuthatch-desk: the window could not be created");
                std::process::exit(1);
            })
            .release();
        engine.load(&QUrl::from("qrc:/qt/qml/desk/app/qml/Main.qml"));
    }

    match app.as_mut() {
        Some(app) => ExitCode::from(u8::try_from(app.exec()).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}
