//! The client against bodies recorded from a real nest, served over real HTTP.

use std::time::Duration;

use mock_nest::{MockNest, fixtures};
use nest_client::{
    Client, Error, Limits,
    poll::{Poller, Request},
    sql::Cell,
    types::SqlAccess,
};

const TRANSFER: &str = "fiat_token_v2_2__transfer";

fn client() -> Client {
    Client::new(Duration::from_secs(5), Limits::default()).unwrap()
}

/// A loopback URL nothing listens on: a port bound and then released.
fn closed_port() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

fn transfers() -> Request {
    Request {
        table: Some(TRANSFER.into()),
        rows: 50,
    }
}

#[test]
fn a_healthy_nest_reads_as_it_was_recorded() {
    let nest = MockNest::recorded();
    let snapshot = Poller::new(client(), nest.url()).poll(&transfers());

    let ready = snapshot.ready.unwrap();
    assert!(ready.ready && !ready.stalled);
    assert_eq!(ready.tip, Some(26_096_543));
    assert_eq!(ready.last_block, 26_096_410);
    assert_eq!(ready.lag_blocks, Some(133));
    assert_eq!(ready.sealed_through, 0);
    assert_eq!(ready.version.as_deref(), Some("3.13.3"));
    assert_eq!(ready.freshness.unwrap().poll_interval_secs, Some(12));

    let identity = snapshot.identity.unwrap();
    assert_eq!(identity.nest_name.as_deref(), Some("usdc"));
    assert_eq!(identity.chain.as_deref(), Some("mainnet"));
    assert_eq!(identity.tables.tables.len(), 17);
    assert_eq!(identity.sql, SqlAccess::Open);
    assert_eq!(identity.tables.tables[0].group(), "fiat_token_v2_2");
    assert_eq!(identity.tables.tables[0].short_name(), "approval");

    assert_eq!(snapshot.hot_rows.unwrap(), Some(17_538));
    let metrics = snapshot.metrics.unwrap();
    assert_eq!(metrics["nuthatch_rpc_requests_total"], 69.0);
    assert_eq!(metrics["nuthatch_rows_decoded_total"], 17_538.0);
    assert!(!snapshot.restarted);
    assert!(snapshot.roster.is_none());

    let selection = snapshot.selection.unwrap().unwrap();
    assert_eq!(selection.table, TRANSFER);
    assert_eq!(selection.rows, Some(15_958));
    assert_eq!(selection.latest_block, Some(26_096_410));
    // The nest sends a row's fields alphabetically; the preview puts them back in catalogue order
    // and leaves out the implicit columns and the `_dec` and `_overflow` companions.
    assert_eq!(
        selection.newest.columns,
        ["block_number", "from", "to", "value"]
    );
    assert_eq!(
        selection.newest.rows[0],
        [
            Cell::Number("26096410".into()),
            Cell::Text("0x1470e1d18200b8733b88ad867259d4f1f1e0c7c2".into()),
            Cell::Text("0xfb796a8af7ff4d9ff37e9f6afac8e21bafe68e61".into()),
            Cell::Text("321000000".into()),
        ]
    );
    assert_eq!(selection.newest.rows.len(), 3);
}

#[test]
fn a_stalled_nest_is_read_through_its_503() {
    let nest = MockNest::recorded();
    nest.route("/ready", 503, fixtures::READY_503);
    let ready = client().ready(&nest.url()).unwrap();
    assert!(!ready.ready);
    assert!(ready.stalled && ready.initial_poll_failed);
    // Null in the recorded body: the nest has not polled once.
    assert_eq!(ready.seconds_since_poll, None);
    assert_eq!(ready.version.as_deref(), Some("3.13.3"));
}

#[test]
fn any_other_failure_of_ready_is_reported() {
    let nest = MockNest::recorded();
    nest.route("/ready", 500, fixtures::READY);
    assert_eq!(client().ready(&nest.url()), Err(Error::Status(500)));
    nest.route("/ready", 200, "<html>a captive portal</html>");
    assert_eq!(client().ready(&nest.url()), Err(Error::Unreadable));
}

#[test]
fn a_refused_statement_comes_back_in_the_nests_words() {
    let nest = MockNest::recorded();
    let Err(Error::Refused(reason)) = client().sql(&nest.url(), "SELECT nope FROM nowhere") else {
        panic!("expected a refusal");
    };
    assert!(reason.starts_with("failed to prepare query: Catalog Error: Table with name nowhere"));
    assert!(reason.ends_with("Call the `schema` tool for the list of tables."));
}

#[test]
fn a_body_over_the_cap_is_refused_unread() {
    let nest = MockNest::recorded();
    let limits = Limits {
        body_bytes: 1024,
        sql_body_bytes: 64,
        ..Limits::default()
    };
    let client = Client::new(Duration::from_secs(5), limits).unwrap();
    // `/metrics` is 15 KiB in the recording, `/ready` under one.
    assert_eq!(client.metrics(&nest.url()), Err(Error::TooLarge(1024)));
    assert!(client.ready(&nest.url()).is_ok());
    assert_eq!(
        client.sql(&nest.url(), "SELECT 1"),
        Err(Error::TooLarge(64))
    );
}

#[test]
fn nothing_listening_is_a_connection_failure() {
    assert_eq!(client().ready(&closed_port()), Err(Error::Connect));
}

#[test]
fn a_nest_that_does_not_answer_times_out() {
    let nest = MockNest::recorded();
    nest.delay("/ready", Duration::from_secs(3));
    let client = Client::new(Duration::from_millis(300), Limits::default()).unwrap();
    assert_eq!(client.ready(&nest.url()), Err(Error::Timeout));
}

#[test]
fn a_runtime_root_reports_the_nests_it_mounts() {
    let nest = MockNest::start();
    nest.route("/ready", 200, "{\"ready\":true}");
    nest.route(
        "/nests",
        200,
        "{\"runtime\":\"nuthatch\",\"nests\":[{\"name\":\"usdc\",\"base_path\":\"/n/usdc\",\"health\":\"ok\"},{\"name\":\"dai\"}]}",
    );
    let snapshot = Poller::new(client(), nest.url()).poll(&Request::default());
    let roster = snapshot.roster.unwrap();
    assert_eq!(roster.nests[0].path(), "/n/usdc");
    assert_eq!(roster.nests[1].path(), "/dai");
    // Its `/ready` has no heights and would read as a ready nest at block zero.
    assert!(snapshot.ready.is_err());
}

#[test]
fn the_catalogue_is_fetched_once_and_again_after_a_restart() {
    let nest = MockNest::recorded();
    let mut poller = Poller::new(client(), nest.url());
    assert!(!poller.poll(&Request::default()).restarted);
    assert!(!poller.poll(&Request::default()).restarted);
    assert_eq!(nest.hits("/tables"), 1);

    // A new process starts its counters again.
    nest.route(
        "/metrics",
        200,
        "nuthatch_rpc_requests_total 2\nnuthatch_rows_decoded_total 17538\n",
    );
    nest.route("/tables", 200, "{\"count\":0,\"tables\":[]}");
    let snapshot = poller.poll(&Request::default());
    assert!(snapshot.restarted);
    assert_eq!(nest.hits("/tables"), 2);
    assert!(snapshot.identity.unwrap().tables.tables.is_empty());
    assert!(!poller.poll(&Request::default()).restarted);
}

#[test]
fn a_closed_sql_surface_is_not_asked_for_a_preview() {
    let nest = MockNest::recorded();
    nest.route(
        "/queries",
        200,
        "{\"free_form\":false,\"sql\":\"named\",\"queries\":[{\"name\":\"top_holders\"}]}",
    );
    let snapshot = Poller::new(client(), nest.url()).poll(&transfers());
    assert!(snapshot.selection.is_none());
    assert_eq!(nest.hits("/sql"), 0);
    assert_eq!(
        snapshot.identity.unwrap().sql,
        SqlAccess::Closed {
            mode: "named".into(),
            named: vec!["top_holders".into()],
        }
    );
}

#[test]
fn a_table_the_nest_does_not_have_is_not_previewed() {
    let nest = MockNest::recorded();
    let request = Request {
        table: Some("no_such_table".into()),
        rows: 50,
    };
    assert!(
        Poller::new(client(), nest.url())
            .poll(&request)
            .selection
            .is_none()
    );
    assert_eq!(nest.hits("/sql"), 0);
}

#[test]
fn a_nest_that_is_down_fails_every_endpoint_without_panicking() {
    let snapshot = Poller::new(client(), closed_port()).poll(&transfers());
    assert_eq!(snapshot.ready, Err(Error::Connect));
    assert_eq!(snapshot.metrics, Err(Error::Connect));
    assert_eq!(snapshot.hot_rows, Err(Error::Connect));
    assert!(snapshot.identity.is_none() && snapshot.selection.is_none());
}

#[test]
fn a_nest_that_does_not_answer_ready_is_asked_nothing_else() {
    let nest = MockNest::recorded();
    let client = Client::new(Duration::from_millis(300), Limits::default()).unwrap();
    let mut poller = Poller::new(client, nest.url());
    assert!(poller.poll(&transfers()).ready.is_ok());
    let asked = |path: &str| nest.hits(path);
    let before = (
        asked("/metrics"),
        asked("/"),
        asked("/sql"),
        asked("/tables"),
    );

    nest.delay("/ready", Duration::from_secs(2));
    let snapshot = poller.poll(&transfers());
    assert_eq!(snapshot.ready, Err(Error::Timeout));
    assert_eq!(snapshot.metrics, Err(Error::Timeout));
    // One request waited out, not five.
    assert_eq!(
        (
            asked("/metrics"),
            asked("/"),
            asked("/sql"),
            asked("/tables")
        ),
        before
    );
    // What was learned while the nest answered is kept for when it does again.
    assert_eq!(snapshot.identity.unwrap().tables.tables.len(), 17);
    assert!(snapshot.elapsed < Duration::from_millis(1500));
}
