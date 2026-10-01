//! The real QObjects, headless, against a mock nest.
//!
//! Each scenario loads a few lines of QML that create the objects under test, drive them, and call
//! `Qt.exit(0)` when what they show is right. A scenario that never gets there exits 1 from its own
//! timer and prints what it saw. The Rust side then checks what QML cannot see: threads, and what
//! the mock nest was asked for.
//!
//! `cargo test -p desk --test live` runs them all. A name filters, as with the standard harness.
//! `-- --scenario screenshot` with `DESK_SHOT_URL`, `DESK_SHOT_OUT` and optionally
//! `DESK_SHOT_SECTION` renders the real window against a real nest and saves it as a PNG, and
//! `-- --scenario figures` prints what `NestStatus` shows for the nest at `DESK_SHOT_URL`.

// On macOS the linker remarks that both cxx-qt build scripts gave it Qt's rpath, and that
// Homebrew's Qt was built for a newer macOS than the one in `.cargo/config.toml`.
#![allow(linker_messages)]

// Nothing here names a Rust item from the bridge, and without a reference its objects are not linked.
extern crate desk_bridge;

use std::{
    process::{Command, ExitCode},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use cxx_qt_lib::{QByteArray, QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};
use desk_core::feed::Hub;
use mock_nest::{MockNest, fixtures};

type Scenario = (&'static str, fn() -> Result<(), String>);

const SCENARIOS: &[Scenario] = &[
    ("status_of_a_healthy_nest", status_of_a_healthy_nest),
    ("status_of_a_stalled_nest", status_of_a_stalled_nest),
    (
        "status_of_a_nest_that_is_down",
        status_of_a_nest_that_is_down,
    ),
    (
        "a_url_that_is_not_http_is_refused",
        a_url_that_is_not_http_is_refused,
    ),
    ("destroyed_mid_poll", destroyed_mid_poll),
    ("queue_after_destruction", queue_after_destruction),
    ("the_feed_slides_in_place", the_feed_slides_in_place),
    (
        "a_statement_runs_sorts_copies_is_refused_and_cancels",
        sql_workbench,
    ),
    ("a_metric_keeps_its_history", a_metric_keeps_its_history),
    ("two_nests_one_stopped", two_nests_one_stopped),
    (
        "a_result_is_copied_out_as_csv",
        a_result_is_copied_out_as_csv,
    ),
    (
        "a_default_text_item_would_fetch_a_nests_markup",
        a_default_text_item_would_fetch,
    ),
    (
        "the_window_shows_a_nests_markup_as_text",
        the_window_shows_markup_as_text,
    ),
];

/// Not tests: run by name only.
const TOOLS: &[Scenario] = &[("screenshot", screenshot), ("figures", figures)];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, name] = args.as_slice()
        && flag == "--scenario"
    {
        return child(name);
    }
    let filters: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-')).collect();
    let (mut ran, mut failed) = (0, 0);
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        // QML reports a binding that failed or a type that did not resolve as a warning and
        // carries on. From this project's own files, either is a defect.
        let own_warning = stderr
            .lines()
            .find(|line| line.contains("qrc:/qt/qml/desk/"));
        if output.status.success() && own_warning.is_none() {
            println!("test {name} ... ok");
        } else {
            failed += 1;
            println!("test {name} ... FAILED ({})", output.status);
            if let Some(warning) = own_warning {
                println!("a warning from the project's own QML: {warning}");
            }
            print!("{}{stderr}", String::from_utf8_lossy(&output.stdout));
        }
    }
    println!("\n{ran} scenarios, {failed} failed");
    // A filter that matches nothing must not read as a pass.
    if failed == 0 && ran > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn child(name: &str) -> ExitCode {
    let known = SCENARIOS
        .iter()
        .chain(TOOLS)
        .find(|(known, _)| *known == name);
    let Some((_, scenario)) = known else {
        eprintln!("no scenario called {name}");
        return ExitCode::FAILURE;
    };
    // A scenario whose QML never loads has no timer to end it.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(40));
        eprintln!("the scenario did not finish in 40 seconds");
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
    run_then(qml, || Ok(()))
}

/// As [`run`], then `check` while the QML's objects are still alive. Counting pollers after the
/// engine has gone would count nothing: tearing it down destroys every object and ends them all.
fn run_then(qml: &str, check: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    cxx_qt::init_crate!(desk_bridge);
    cxx_qt::init_crate!(desk);
    cxx_qt::init_qml_module!("desk.app");
    let mut app = QGuiApplication::new();
    QQuickStyle::set_style(&QString::from("Fusion"));
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
        check()
    } else {
        Err(format!("the scenario's QML exited {code}"))
    }
}

fn workers() -> Result<usize, String> {
    Hub::global()
        .map(Hub::workers)
        .map_err(|error| error.to_string())
}

/// A loopback URL nothing listens on: a port bound and then released.
fn closed_port() -> Result<String, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(format!(
        "http://{}",
        listener.local_addr().map_err(|e| e.to_string())?
    ))
}

// ---- NestStatus ----------------------------------------------------------------------------

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
    run(&status_scenario(
        &closed_port()?,
        r#"status.state === NestStatus.Connecting && root.failure === "cannot connect"
            && status.tip === -1 && status.problems.indexOf("/ready: cannot connect") >= 0"#,
    ))
}

fn a_url_that_is_not_http_is_refused() -> Result<(), String> {
    let qml = status_scenario(
        "file:///etc/passwd",
        r#"root.failure === "scheme 'file' is not http or https"
            && status.problems === root.failure && status.state === NestStatus.Connecting"#,
    );
    run_then(&qml, || match workers()? {
        0 => Ok(()),
        workers => Err(format!("a refused URL started {workers} poller(s)")),
    })
}

// ---- Lifetime ------------------------------------------------------------------------------

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
    run_then(&destruction_scenario(&nest.url()), || {
        if nest.hits("/ready") != 1 {
            return Err(format!(
                "expected one poll in flight, saw {}",
                nest.hits("/ready")
            ));
        }
        if workers()? != 0 {
            return Err(format!("{} poller(s) outlived the object", workers()?));
        }
        // The object unsubscribed as it went, so the poller had nobody to deliver to.
        match desk_bridge::probe::refused() {
            0 => Ok(()),
            refused => Err(format!(
                "{refused} deliveries were attempted after an orderly unsubscribe"
            )),
        }
    })
}

fn queue_after_destruction() -> Result<(), String> {
    let nest = slow_nest();
    // The sink now outlives the object, so the poller will queue onto a destroyed QObject.
    desk_bridge::probe::leak_next_subscription();
    run_then(&destruction_scenario(&nest.url()), || {
        if desk_bridge::probe::refused() != 1 {
            return Err(format!(
                "expected the queue to refuse one delivery, it refused {}",
                desk_bridge::probe::refused()
            ));
        }
        match workers()? {
            0 => Ok(()),
            workers => Err(format!(
                "{workers} poller(s) kept polling for a destroyed object"
            )),
        }
    })
}

// ---- TableList and the feed ----------------------------------------------------------------

/// The recorded feed after one new transfer: a new row on top, the old third row gone.
const MOVED_ROWS: &str = r#"{"count":3,"truncated":false,"degraded":false,"tip_unavailable":false,"rows":[
{"block_number":26096411,"from":"0x00000000000000000000000000000000000000aa","log_index":3,"to":"0x00000000000000000000000000000000000000bb","value":"42"},
{"block_number":26096410,"from":"0x1470e1d18200b8733b88ad867259d4f1f1e0c7c2","log_index":949,"to":"0xfb796a8af7ff4d9ff37e9f6afac8e21bafe68e61","value":"321000000"},
{"block_number":26096410,"from":"0x06fd4ba7973a0d39a91734bbc35bc2bcaa99e3b0","log_index":893,"to":"0xee7ae85f2fe2239e27d9c1e23fffe168d63b4055","value":"1000000000000"}]}"#;

fn the_feed_slides_in_place() -> Result<(), String> {
    let nest = MockNest::recorded();
    let moved = nest.flag("/moved");
    nest.sql(move |statement| {
        if statement.contains("count(*)") {
            (200, fixtures::SQL_COUNTS.to_owned())
        } else if moved.load(Ordering::SeqCst) {
            (200, MOVED_ROWS.to_owned())
        } else {
            (200, fixtures::SQL_ROWS.to_owned())
        }
    });
    let url = nest.url();
    let qml = format!(
        r#"
import QtQuick
import desk.bridge

Item {{
    id: root
    property int step: 0
    property int resets: 0
    property var inserted: []
    property var removed: []

    NestStatus {{ id: status }}
    TableList {{ id: tables; url: status.url; onChanged: root.advance() }}
    ResultModel {{ id: feed; resultKey: tables.feedKey; onShapeChanged: root.advance() }}
    Repeater {{
        id: rows
        model: tables
        Item {{
            required property string table
            required property string group
            required property string name
            required property bool call
        }}
    }}
    Connections {{
        target: feed
        function onModelReset() {{ root.resets += 1 }}
        function onRowsInserted(parent, first, last) {{ root.inserted = root.inserted.concat([[first, last]]) }}
        function onRowsRemoved(parent, first, last) {{ root.removed = root.removed.concat([[first, last]]) }}
    }}

    function cell(row, column) {{ return feed.data(feed.index(row, column), 0) }}

    function advance() {{
        if (step === 0 && tables.count === 17 && rows.count === 17 && tables.selected === 0
                && tables.selectedTable === "fiat_token_v2_2__approval" && tables.sqlOpen
                && rows.itemAt(0).name === "approval" && rows.itemAt(0).group === "fiat_token_v2_2"
                && !rows.itemAt(0).call && rows.itemAt(14).table === "fiat_token_v2_2__transfer") {{
            step = 1
            tables.select(14)
        }} else if (step === 1 && tables.selected === 14
                && tables.selectedTable === "fiat_token_v2_2__transfer"
                && tables.selectedRows === 15958 && tables.selectedLatestBlock === 26096410
                && feed.rows === 3 && feed.columnNames.join(",") === "block_number,from,to,value"
                && cell(0, 0) === "26096410" && cell(0, 3) === "321000000"
                && cell(2, 3) === "603568175") {{
            step = 2
            resets = 0
            inserted = []
            removed = []
            // Tell the mock a new transfer has arrived, then poll.
            let request = new XMLHttpRequest()
            request.onreadystatechange = function() {{
                if (request.readyState === XMLHttpRequest.DONE)
                    status.refresh()
            }}
            request.open("GET", "{url}/moved")
            request.send()
        }} else if (step === 2 && feed.rows === 3 && cell(0, 3) === "42") {{
            let slid = resets === 0 && JSON.stringify(removed) === "[[2,2]]"
                && JSON.stringify(inserted) === "[[0,0]]" && cell(0, 0) === "26096411"
                && cell(1, 3) === "321000000" && cell(2, 3) === "1000000000000"
            if (!slid)
                console.error("the feed did not slide: resets", resets, "removed",
                    JSON.stringify(removed), "inserted", JSON.stringify(inserted))
            Qt.exit(slid ? 0 : 1)
        }}
    }}

    Timer {{ interval: 20; running: true; onTriggered: status.connectTo("{url}") }}
    Timer {{
        interval: 8000; running: true
        onTriggered: {{
            console.error("stuck at step", root.step, "count", tables.count, "selected",
                tables.selected, tables.selectedTable, "rows", tables.selectedRows, "latest",
                tables.selectedLatestBlock, "feed", feed.rows, feed.columnNames.join(","),
                "key", tables.feedKey, "first", root.cell(0, 0), root.cell(0, 3))
            Qt.exit(1)
        }}
    }}
}}
"#
    );
    run_then(&qml, || match workers()? {
        // Three objects on one nest share one poller.
        1 => Ok(()),
        workers => Err(format!("expected one poller for the nest, found {workers}")),
    })
}

// ---- SqlQuery and ResultModel --------------------------------------------------------------

fn sql_workbench() -> Result<(), String> {
    let nest = MockNest::recorded();
    nest.slow("held back", Duration::from_millis(600));
    let url = nest.url();
    run(&format!(
        r#"
import QtQuick
import desk.bridge

Item {{
    id: root
    property int step: 0
    property int finishes: 0
    property int failures: 0
    property string lastFailure: ""

    SqlQuery {{
        id: query
        url: "{url}"
        onFinished: {{ root.finishes += 1; root.advance() }}
        onFailed: function(message) {{ root.failures += 1; root.lastFailure = message; root.advance() }}
    }}
    ResultModel {{ id: results; resultKey: query.resultKey }}

    function cell(row, column) {{ return results.data(results.index(row, column), 0) }}
    function fail(what) {{
        console.error("failed at:", what, "step", step, "finishes", finishes, "failures", failures,
            "lastFailure", lastFailure, "running", query.running, "rows", results.rows,
            "rowCount", query.rowCount, "error", query.error, "columns",
            results.columnNames.join(","), "sort", results.sortColumn, results.sortDescending,
            "first", cell(0, 10))
        Qt.exit(1)
    }}

    function advance() {{
        if (step === 1 && finishes === 1) {{
            if (!(query.rowCount === 3 && !query.running && query.error === "" && failures === 0
                    && results.rows === 3 && results.columnNames.length === 13
                    && results.columnNames[10] === "value" && results.sortColumn === -1
                    && cell(0, 10) === "321000000" && cell(1, 10) === "1000000000000"))
                return fail("the first run")
            // As text "1000000000000" would sort first. As a number it is the largest.
            results.sortBy(10)
            if (!(cell(0, 10) === "321000000" && cell(1, 10) === "603568175"
                    && cell(2, 10) === "1000000000000" && results.sortColumn === 10
                    && !results.sortDescending))
                return fail("ascending")
            results.sortBy(10)
            if (!(cell(0, 10) === "1000000000000" && cell(2, 10) === "321000000"
                    && results.sortDescending))
                return fail("descending")
            let csv = results.csv().split("\n")
            if (!(csv.length === 5 && csv[4] === ""
                    && csv[0] === "_seq,address,block_hash,block_number,block_timestamp,from,log_index,table,to,tx_hash,value,value_dec,value_overflow"
                    && csv[1].indexOf(",1000000000000,1000000000000,false") > 0
                    && csv[3].indexOf(",321000000,321000000,false") > 0))
                return fail("csv")
            results.sortBy(99)
            results.clearSort()
            if (!(cell(0, 10) === "321000000" && cell(1, 10) === "1000000000000"
                    && results.sortColumn === -1))
                return fail("unsorted")
            step = 2
            query.run("SELECT nope FROM nowhere")
        }} else if (step === 2 && failures === 1) {{
            if (!(lastFailure.indexOf("failed to prepare query: Catalog Error: Table with name nowhere does not exist!") === 0
                    && lastFailure.indexOf("hint: no table `nowhere`.") > 0
                    && query.error === lastFailure && !query.running && finishes === 1
                    && results.rows === 3))
                return fail("the refusal")
            step = 3
            query.run("SELECT 1 -- held back")
            if (!(query.running && query.error === ""))
                return fail("running")
            query.cancel()
            if (query.running)
                return fail("cancel")
            cancelled.start()
        }} else if (step === 4 && finishes === 2) {{
            Qt.exit(query.rowCount === 3 && failures === 1 ? 0 : 1)
        }}
    }}

    // The cancelled statement is answered 600 ms after it was sent. Nothing may come of it.
    Timer {{
        id: cancelled
        interval: 1300
        onTriggered: {{
            if (root.finishes !== 1 || root.failures !== 1 || query.running)
                return root.fail("an answer to a cancelled statement was applied")
            root.step = 4
            query.run("SELECT * FROM t")
        }}
    }}
    Timer {{ interval: 20; running: true; onTriggered: {{ root.step = 1; query.run("SELECT * FROM t") }} }}
    Timer {{ interval: 8000; running: true; onTriggered: root.fail("timed out") }}
}}
"#
    ))?;
    // The first run, the refusal, the cancelled statement and the last run.
    match nest.hits("/sql") {
        4 => Ok(()),
        hits => Err(format!(
            "expected four statements to reach the nest, saw {hits}"
        )),
    }
}

// ---- MetricSeries --------------------------------------------------------------------------

fn a_metric_keeps_its_history() -> Result<(), String> {
    let nest = MockNest::recorded();
    let url = nest.url();
    run(&format!(
        r#"
import QtQuick
import desk.bridge

Item {{
    id: root
    property int inserts: 0
    property int removals: 0
    property int polled: 0

    NestStatus {{ id: status }}
    MetricSeries {{ id: lag; url: status.url; metric: "lag"; onChanged: root.advance() }}
    MetricSeries {{ id: rate; url: status.url; metric: "rate:nuthatch_rpc_requests_total" }}
    MetricSeries {{ id: unknown; url: status.url; metric: "nonsense" }}
    Connections {{
        target: lag
        function onRowsInserted(parent, first, last) {{ root.inserts += 1 }}
        function onRowsRemoved(parent, first, last) {{ root.removals += last - first + 1 }}
    }}

    function advance() {{
        if (lag.count > polled && lag.count < 3) {{
            polled = lag.count
            status.refresh()
        }} else if (lag.count === 3 && !settle.running && polled < 3) {{
            polled = 3
            // The other two series are told of the same poll just after this one.
            settle.start()
        }}
    }}

    Timer {{
        id: settle
        interval: 200
        onTriggered: {{
            let line = lag.polyline(100, 50)
            let ok = lag.known && lag.latest === 133 && lag.peak === 133 && root.inserts === 3
                && lag.data(lag.index(2, 0), 257) === 133 && lag.windowSecs === 3600
                && line.length === 3 && line[2].x === 100 && line[2].y === 0
                // A rate has nothing to be a rate against on its first reading.
                && rate.known && rate.count === 2 && rate.latest === 0
                && !unknown.known && unknown.count === 0
            lag.clear()
            ok = ok && lag.count === 0 && root.removals === 3 && lag.polyline(100, 50).length === 0
            if (!ok)
                console.error("lag", lag.count, lag.latest, lag.peak, lag.known, "inserts",
                    root.inserts, "removals", root.removals, "line", line.length, "rate",
                    rate.count, rate.latest, rate.known, "unknown", unknown.count, unknown.known)
            Qt.exit(ok ? 0 : 1)
        }}
    }}
    Timer {{ interval: 20; running: true; onTriggered: status.connectTo("{url}") }}
    Timer {{
        interval: 8000; running: true
        onTriggered: {{ console.error("timed out with", lag.count, "points"); Qt.exit(1) }}
    }}
}}
"#
    ))?;
    match nest.hits("/ready") {
        3 => Ok(()),
        hits => Err(format!("expected three polls, saw {hits}")),
    }
}

// ---- The window ----------------------------------------------------------------------------

fn two_nests_one_stopped() -> Result<(), String> {
    let (stopping, steady) = (
        Arc::new(MockNest::recorded()),
        Arc::new(MockNest::recorded()),
    );
    let stop = stopping.flag("/stop");
    let steady_polls_at_stop = Arc::new(AtomicUsize::new(usize::MAX));
    {
        let (stopping, steady, polls) = (
            Arc::clone(&stopping),
            Arc::clone(&steady),
            Arc::clone(&steady_polls_at_stop),
        );
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            stopping.shutdown();
            polls.store(steady.hits("/ready"), Ordering::SeqCst);
        });
    }
    let (stopping_url, steady_url) = (stopping.url(), steady.url());
    let qml = format!(
        r#"
import QtQuick
import desk.bridge
import desk.app

Main {{
    id: root
    property int step: 0
    readonly property var a: root.pages.count === 2 ? root.pages.itemAt(0) : null
    readonly property var b: root.pages.count === 2 ? root.pages.itemAt(1) : null

    nestNames: ["stopping", "steady"]
    nestUrls: ["{stopping_url}", "{steady_url}"]
    nestNotes: ["", ""]

    function healthy(page) {{
        return page.status.state === NestStatus.Live && page.status.problems === ""
            && page.tables.count === 17 && page.feed.rows === 3
    }}

    Timer {{
        interval: 100; repeat: true; running: true
        onTriggered: {{
            if (!root.a || !root.b)
                return
            if (root.step === 0 && root.healthy(root.a) && root.healthy(root.b)) {{
                root.step = 1
                let request = new XMLHttpRequest()
                request.onreadystatechange = function() {{
                    if (request.readyState === XMLHttpRequest.DONE)
                        root.step = 2
                }}
                request.open("GET", "{stopping_url}/stop")
                request.send()
            }} else if (root.step === 2) {{
                root.a.status.refresh()
                root.b.status.refresh()
                // The stopped nest keeps its last figures and says they are stale.
                if (root.a.status.state === NestStatus.Stale && root.a.status.indexed === 26096410
                        && root.a.status.problems.indexOf("/ready: cannot connect") >= 0)
                    root.step = 3
            }} else if (root.step === 3) {{
                root.step = 4
                root.b.status.refresh()
            }} else if (root.step >= 4 && root.step < 8) {{
                root.step += 1
            }} else if (root.step === 8) {{
                Qt.exit(root.healthy(root.b) && root.a.status.state === NestStatus.Stale ? 0 : 1)
            }}
        }}
    }}
    Timer {{
        interval: 15000; running: true
        onTriggered: {{
            console.error("stuck at step", root.step, "a", root.a ? root.a.status.state : "none",
                root.a ? root.a.status.problems : "", "b", root.b ? root.b.status.state : "none",
                root.b ? root.b.status.problems : "", root.b ? root.b.tables.count : -1,
                root.b ? root.b.feed.rows : -1)
            Qt.exit(1)
        }}
    }}
}}
"#
    );
    run_then(&qml, || {
        let (at_stop, at_end) = (
            steady_polls_at_stop.load(Ordering::SeqCst),
            steady.hits("/ready"),
        );
        if at_end <= at_stop {
            return Err(format!(
                "the steady nest was polled {at_stop} times when the other stopped and {at_end} by the end"
            ));
        }
        match workers()? {
            // The stopped nest's poller keeps trying: it may come back.
            2 => Ok(()),
            workers => Err(format!("expected a poller per nest, found {workers}")),
        }
    })
}

fn a_result_is_copied_out_as_csv() -> Result<(), String> {
    let nest = MockNest::recorded();
    let url = nest.url();
    run(&format!(
        r#"
import QtQuick
import desk.bridge
import desk.app

Main {{
    id: main
    nestNames: ["nest"]
    nestUrls: ["{url}"]
    nestNotes: [""]

    // Where the clipboard is read back from.
    TextEdit {{ id: pasted; visible: false; textFormat: TextEdit.PlainText }}

    Timer {{
        interval: 100; repeat: true; running: true
        property int step: 0
        onTriggered: {{
            let page = main.pages.itemAt(0)
            if (!page)
                return
            if (step === 0 && page.status.state === NestStatus.Live) {{
                step = 1
                page.query.run("SELECT * FROM t")
            }} else if (step === 1 && page.results.rows === 3) {{
                step = 2
                page.results.sortBy(10)
                page.copy(page.results.csv())
                pasted.paste()
                let lines = pasted.text.split("\n")
                let ok = lines.length === 5 && lines[0].indexOf("_seq,address,block_hash") === 0
                    && lines[1].indexOf(",321000000,321000000,false") > 0
                    && lines[3].indexOf(",1000000000000,1000000000000,false") > 0
                if (!ok)
                    console.error("the clipboard held", lines.length, "lines:", pasted.text)
                Qt.exit(ok ? 0 : 1)
            }}
        }}
    }}
    Timer {{ interval: 8000; running: true; onTriggered: {{ console.error("timed out"); Qt.exit(1) }} }}
}}
"#
    ))
}

/// A nest that names itself in markup which, rendered, fetches `/fetched` from the nest.
fn hostile_nest() -> MockNest {
    let nest = MockNest::recorded();
    let image = format!("<img src='{}/fetched'><b>bold</b>", nest.url());
    nest.route(
        "/nest",
        200,
        format!("{{\"name\":\"{image}\",\"chain\":\"{image}\"}}"),
    );
    nest
}

/// The control for the test below: proves that the markup is live, so that not fetching it means
/// something.
fn a_default_text_item_would_fetch() -> Result<(), String> {
    let nest = hostile_nest();
    let url = nest.url();
    run(&format!(
        r#"
import QtQuick
import QtQuick.Window
import desk.bridge

Window {{
    visible: true
    width: 400
    height: 100
    NestStatus {{ id: status }}
    // No `textFormat`: this is the mistake `PlainLabel` exists to prevent.
    Text {{ text: status.nestName }}
    Timer {{ interval: 20; running: true; onTriggered: status.connectTo("{url}") }}
    Timer {{ interval: 2500; running: true; onTriggered: Qt.exit(status.nestName !== "" ? 0 : 1) }}
}}
"#
    ))?;
    match nest.hits("/fetched") {
        0 => Err("a default Text did not fetch the image, so the next test proves nothing".into()),
        _ => Ok(()),
    }
}

fn the_window_shows_markup_as_text() -> Result<(), String> {
    let nest = hostile_nest();
    let url = nest.url();
    run(&format!(
        r#"
import QtQuick
import desk.bridge
import desk.app

Main {{
    id: main
    nestNames: ["<img src='{url}/fetched'>"]
    nestUrls: ["{url}"]
    nestNotes: ["<img src='{url}/fetched'>"]
    configProblem: "<img src='{url}/fetched'>"
    // A page with a note waits to be told its forward is open.
    Timer {{
        interval: 500; running: true
        onTriggered: {{
            let page = main.pages.itemAt(0)
            if (page.status.url !== "" || page.status.state !== NestStatus.Connecting)
                Qt.exit(2)
            page.status.connectTo(page.url)
        }}
    }}
    Timer {{
        interval: 3000; running: true
        onTriggered: {{
            let page = main.pages.itemAt(0)
            Qt.exit(page && page.status.nestName.indexOf("<img") === 0
                && page.status.state === NestStatus.Live ? 0 : 1)
        }}
    }}
}}
"#
    ))?;
    match nest.hits("/fetched") {
        0 => Ok(()),
        hits => Err(format!("the window fetched a nest's markup {hits} time(s)")),
    }
}

/// Prints what `NestStatus` shows for the nest at `DESK_SHOT_URL` after five seconds, for setting
/// beside the terminal client on the same nest.
fn figures() -> Result<(), String> {
    let url = std::env::var("DESK_SHOT_URL").map_err(|_| "DESK_SHOT_URL is not set".to_owned())?;
    // A condition that never holds: the scenario's own timeout prints the figures.
    let _ = run(&status_scenario(&url, "false"));
    Ok(())
}

/// Renders the real window against `DESK_SHOT_URL` and saves it to `DESK_SHOT_OUT`.
fn screenshot() -> Result<(), String> {
    let var = |name: &str| std::env::var(name).map_err(|_| format!("{name} is not set"));
    let (url, out) = (var("DESK_SHOT_URL")?, var("DESK_SHOT_OUT")?);
    let section = std::env::var("DESK_SHOT_SECTION").unwrap_or_else(|_| "0".into());
    let statement = std::env::var("DESK_SHOT_SQL").unwrap_or_default();
    let wait = std::env::var("DESK_SHOT_WAIT_MS").unwrap_or_else(|_| "6000".into());
    run(&format!(
        r#"
import QtQuick
import desk.bridge
import desk.app

Main {{
    id: main
    nestNames: ["nest"]
    nestUrls: ["{url}"]
    nestNotes: [""]
    Timer {{
        interval: 1500; running: true
        onTriggered: {{
            let page = main.pages.itemAt(0)
            page.section = {section}
            if ("{statement}" !== "")
                page.query.run("{statement}")
        }}
    }}
    Timer {{
        interval: {wait}; running: true
        onTriggered: main.shell.grabToImage(function(result) {{
            Qt.exit(result.saveToFile("{out}") ? 0 : 1)
        }})
    }}
}}
"#
    ))
}
