//! The real QObjects, headless, against a mock nest.
//!
//! Each scenario loads a few lines of QML that create the objects under test, drive them, and call
//! `Qt.exit(0)` when what they show is right. A scenario that never gets there exits 1 from its own
//! timer and prints what it saw. The Rust side then checks what QML cannot see: threads.

// Nothing here names a Rust item from the bridge, and without a reference its objects are not linked.
extern crate desk_bridge;

use std::{
    process::{Command, ExitCode},
    time::Duration,
};

use cxx_qt_lib::{QByteArray, QGuiApplication, QQmlApplicationEngine, QUrl};
use desk_core::feed::Hub;
use mock_nest::{MockNest, fixtures};

type Scenario = (&'static str, fn() -> Result<(), String>);

const SCENARIOS: &[Scenario] = &[
    ("status_of_a_healthy_nest", status_of_a_healthy_nest),
    ("status_of_a_stalled_nest", status_of_a_stalled_nest),
    ("status_of_a_nest_that_is_down", status_of_a_nest_that_is_down),
    ("a_url_that_is_not_http_is_refused", a_url_that_is_not_http_is_refused),
    ("destroyed_mid_poll", destroyed_mid_poll),
    ("queue_after_destruction", queue_after_destruction),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, name] = args.as_slice()
        && flag == "--scenario"
    {
        return child(name);
    }
    // Any other argument filters scenarios by name, as the standard harness would.
    let filters: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-')).collect();
    let mut failed = 0;
    let mut ran = 0;
    for (name, _) in SCENARIOS {
        if !filters.is_empty() && !filters.iter().any(|filter| name.contains(filter.as_str())) {
            continue;
        }
        ran += 1;
        let output = std::env::current_exe()
            .and_then(|exe| {
                Command::new(exe)
                    .args(["--scenario", name])
                    .env("QT_QPA_PLATFORM", "offscreen")
                    .output()
            })
            .expect("run the scenario");
        if output.status.success() {
            println!("test {name} ... ok");
        } else {
            failed += 1;
            println!("test {name} ... FAILED ({})", output.status);
            print!("{}", String::from_utf8_lossy(&output.stdout));
            print!("{}", String::from_utf8_lossy(&output.stderr));
        }
    }
    println!("\n{} scenarios, {failed} failed", ran);
    // A filter that matches nothing must not read as a pass.
    if failed == 0 && ran > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn child(name: &str) -> ExitCode {
    let Some((_, scenario)) = SCENARIOS.iter().find(|(known, _)| *known == name) else {
        eprintln!("no scenario called {name}");
        return ExitCode::FAILURE;
    };
    // A scenario whose QML never loads has no timer to end it.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("the scenario did not finish in 30 seconds");
        std::process::exit(3);
    });
    match scenario() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// Loads `qml` and runs the event loop until the QML calls `Qt.exit`.
fn run(qml: &str) -> Result<(), String> {
    cxx_qt::init_crate!(desk_bridge);
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    let Some(mut engine) = engine.as_mut() else {
        return Err("no QML engine".into());
    };
    engine
        .as_mut()
        .on_object_creation_failed(|_, _| {
            eprintln!("the scenario's QML did not load");
            std::process::exit(2);
        })
        .release();
    engine.load_data(&QByteArray::from(qml), &QUrl::from("file:///scenario.qml"));
    let code = app.as_mut().map_or(-1, |app| app.exec());
    if code == 0 {
        Ok(())
    } else {
        Err(format!("the scenario's QML exited {code}"))
    }
}

/// QML that creates a `NestStatus`, connects it to `url`, exits 0 once `passes` holds after a
/// change or a failed poll, and exits 1 after five seconds saying what it saw.
fn status_scenario(url: &str, passes: &str) -> String {
    format!(
        r#"
import QtQuick
import desk.bridge

Item {{
    id: root
    property string failure: ""
    function check() {{
        if ({passes})
            Qt.exit(0)
    }}
    NestStatus {{
        id: status
        onChanged: root.check()
        onPollFailed: function(message) {{ root.failure = message; root.check() }}
    }}
    Timer {{ interval: 20; running: true; onTriggered: status.connectTo("{url}") }}
    Timer {{
        interval: 5000; running: true
        onTriggered: {{
            console.error("state", status.state, "partial", status.partial, "tip", status.tip,
                "indexed", status.indexed, "lag", status.lagBlocks, "sealGap", status.sealGap,
                "version", status.version, "name", status.nestName, "chain", status.chain,
                "hot", status.hotRows, "interval", status.pollIntervalSecs, "insecure",
                status.insecure, "sync", status.syncLabel, "problems", status.problems,
                "failure", root.failure)
            Qt.exit(1)
        }}
    }}
}}
"#
    )
}

fn status_of_a_healthy_nest() -> Result<(), String> {
    let nest = MockNest::recorded();
    run(&status_scenario(
        &nest.url(),
        r#"status.state === NestStatus.Live && !status.partial && status.tip === 26096543
            && status.indexed === 26096410 && status.lagBlocks === 133 && status.sealed === 0
            && status.sealGap === 26096410 && status.version === "3.13.3"
            && status.nestName === "usdc" && status.chain === "mainnet"
            && status.hotRows === 17538 && status.pollIntervalSecs === 12 && !status.insecure
            && status.syncLabel === "133 blocks behind" && status.problems === ""
            && root.failure === """#,
    ))
}

fn status_of_a_stalled_nest() -> Result<(), String> {
    let nest = MockNest::recorded();
    nest.route("/ready", 503, fixtures::READY_503);
    run(&status_scenario(
        &nest.url(),
        r#"status.state === NestStatus.Attention && status.tip === 0 && status.problems === """#,
    ))
}

fn status_of_a_nest_that_is_down() -> Result<(), String> {
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        format!("http://{}", listener.local_addr().map_err(|e| e.to_string())?)
    };
    run(&status_scenario(
        &closed,
        r#"status.state === NestStatus.Connecting && root.failure === "cannot connect"
            && status.tip === -1 && status.problems.indexOf("/ready: cannot connect") >= 0"#,
    ))
}

fn a_url_that_is_not_http_is_refused() -> Result<(), String> {
    run(&status_scenario(
        "file:///etc/passwd",
        r#"root.failure === "scheme 'file' is not http or https"
            && status.problems === root.failure && status.state === NestStatus.Connecting"#,
    ))?;
    match Hub::global().map(Hub::workers) {
        Ok(0) => Ok(()),
        other => Err(format!("a refused URL started a poller: {other:?}")),
    }
}

/// QML that creates a `NestStatus` in a `Loader`, connects it, destroys it after 400 ms while its
/// first poll is still waiting on the nest, and exits 0 at 2.5 s if the object was destroyed.
fn destruction_scenario(url: &str) -> String {
    format!(
        r#"
import QtQuick
import desk.bridge

Item {{
    id: root
    property bool destroyed: false
    Loader {{
        id: loader
        sourceComponent: NestStatus {{
            Component.onDestruction: root.destroyed = true
        }}
    }}
    Timer {{ interval: 20; running: true; onTriggered: loader.item.connectTo("{url}") }}
    Timer {{ interval: 400; running: true; onTriggered: loader.active = false }}
    Timer {{ interval: 2500; running: true; onTriggered: Qt.exit(root.destroyed ? 0 : 1) }}
}}
"#
    )
}

/// A nest whose `/ready` takes 1.2 s, so a poll is in flight when the object goes at 400 ms.
fn slow_nest() -> MockNest {
    let nest = MockNest::recorded();
    nest.delay("/ready", Duration::from_millis(1200));
    nest
}

fn destroyed_mid_poll() -> Result<(), String> {
    let nest = slow_nest();
    run(&destruction_scenario(&nest.url()))?;
    if nest.hits("/ready") != 1 {
        return Err(format!("expected one poll in flight, saw {}", nest.hits("/ready")));
    }
    let hub = Hub::global().map_err(|error| error.to_string())?;
    if hub.workers() != 0 {
        return Err(format!("{} poller(s) outlived the object", hub.workers()));
    }
    // The object unsubscribed as it went, so the poller had nobody to deliver to.
    match desk_bridge::probe::refused() {
        0 => Ok(()),
        refused => Err(format!("{refused} deliveries were attempted after an orderly unsubscribe")),
    }
}

fn queue_after_destruction() -> Result<(), String> {
    let nest = slow_nest();
    // The sink now outlives the object, so the poller will queue onto a destroyed QObject.
    desk_bridge::probe::leak_next_subscription();
    run(&destruction_scenario(&nest.url()))?;
    if desk_bridge::probe::refused() != 1 {
        return Err(format!(
            "expected the queue to refuse one delivery, it refused {}",
            desk_bridge::probe::refused()
        ));
    }
    let hub = Hub::global().map_err(|error| error.to_string())?;
    match hub.workers() {
        0 => Ok(()),
        workers => Err(format!("{workers} poller(s) kept polling for a destroyed object")),
    }
}
