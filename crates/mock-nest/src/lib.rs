//! A stand-in nest for tests: canned bodies over real HTTP, on a loopback port.
//!
//! The bodies in [`fixtures`] were recorded from Nuthatch 3.13.3 on 2026-10-01, indexing USDC on
//! mainnet. `READY_503` is from a second nest started against a dead RPC.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

/// Bodies recorded from a real nest.
pub mod fixtures {
    /// `/ready` of a healthy nest following the tip.
    pub const READY: &str = include_str!("../fixtures/ready.json");
    /// `/ready` of a nest whose first poll failed, served under a 503.
    pub const READY_503: &str = include_str!("../fixtures/ready_503_stalled.json");
    /// `/nest`.
    pub const NEST: &str = include_str!("../fixtures/nest.json");
    /// `/queries`.
    pub const QUERIES: &str = include_str!("../fixtures/queries.json");
    /// `/`.
    pub const ROOT: &str = include_str!("../fixtures/root.json");
    /// `/tables`.
    pub const TABLES: &str = include_str!("../fixtures/tables.json");
    /// `/metrics`.
    pub const METRICS: &str = include_str!("../fixtures/metrics.txt");
    /// `/sql` for a `count(*), max(block_number)` over one table.
    pub const SQL_COUNTS: &str = include_str!("../fixtures/sql_counts.json");
    /// `/sql` for the three newest transfers.
    pub const SQL_ROWS: &str = include_str!("../fixtures/sql_rows.json");
    /// `/sql` refusing a statement over a table that does not exist, served under a 400.
    pub const SQL_REFUSED: &str = include_str!("../fixtures/sql_refused.json");
}

/// A response: status and body.
pub type Response = (u16, String);

type Handler = Box<dyn Fn(&str) -> Response + Send>;

#[derive(Default)]
struct Routes {
    fixed: HashMap<String, Response>,
    delays: HashMap<String, Duration>,
    sql: Option<Handler>,
    hits: HashMap<String, usize>,
}

/// A nest that serves what it is told to.
pub struct MockNest {
    port: u16,
    routes: Arc<Mutex<Routes>>,
}

fn lock(routes: &Mutex<Routes>) -> MutexGuard<'_, Routes> {
    routes.lock().unwrap_or_else(PoisonError::into_inner)
}

impl MockNest {
    /// Starts a nest with no routes: every path answers 404.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener.local_addr().expect("local address").port();
        let routes = Arc::new(Mutex::new(Routes::default()));
        let shared = Arc::clone(&routes);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let routes = Arc::clone(&shared);
                std::thread::spawn(move || serve(stream, &routes));
            }
        });
        Self { port, routes }
    }

    /// Starts a nest serving the recorded bodies of a healthy nest.
    pub fn recorded() -> Self {
        let nest = Self::start();
        nest.route("/ready", 200, fixtures::READY);
        nest.route("/nest", 200, fixtures::NEST);
        nest.route("/queries", 200, fixtures::QUERIES);
        nest.route("/", 200, fixtures::ROOT);
        nest.route("/tables", 200, fixtures::TABLES);
        nest.route("/metrics", 200, fixtures::METRICS);
        nest.sql(|statement| {
            if statement.contains("nowhere") {
                (400, fixtures::SQL_REFUSED.to_owned())
            } else if statement.contains("count(*)") {
                (200, fixtures::SQL_COUNTS.to_owned())
            } else {
                (200, fixtures::SQL_ROWS.to_owned())
            }
        });
        nest
    }

    /// The nest's base URL.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Serves `body` under `status` at `path` from now on.
    pub fn route(&self, path: &str, status: u16, body: impl Into<String>) {
        lock(&self.routes)
            .fixed
            .insert(path.to_owned(), (status, body.into()));
    }

    /// Stops serving `path`: it answers 404 again.
    pub fn remove(&self, path: &str) {
        lock(&self.routes).fixed.remove(path);
    }

    /// Holds every answer at `path` back by `delay`.
    pub fn delay(&self, path: &str, delay: Duration) {
        lock(&self.routes).delays.insert(path.to_owned(), delay);
    }

    /// Answers `/sql` with whatever `handler` makes of the decoded statement.
    pub fn sql(&self, handler: impl Fn(&str) -> Response + Send + 'static) {
        lock(&self.routes).sql = Some(Box::new(handler));
    }

    /// How many requests `path` has received.
    pub fn hits(&self, path: &str) -> usize {
        lock(&self.routes).hits.get(path).copied().unwrap_or(0)
    }
}

fn serve(mut stream: TcpStream, routes: &Mutex<Routes>) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(clone) => clone,
        Err(_) => return,
    });
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|read| read > 2) {
        header.clear();
    }
    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let (delay, (status, body)) = {
        let mut routes = lock(routes);
        *routes.hits.entry(path.to_owned()).or_default() += 1;
        let delay = routes.delays.get(path).copied();
        let response = match (&routes.sql, path) {
            (Some(handler), "/sql") => handler(&statement_of(query)),
            _ => routes
                .fixed
                .get(path)
                .cloned()
                .unwrap_or((404, String::new())),
        };
        (delay, response)
    };
    if let Some(delay) = delay {
        std::thread::sleep(delay);
    }
    let content_type = if body.starts_with(['{', '[']) {
        "application/json"
    } else {
        "text/plain"
    };
    // The client may have gone away during the delay, which is what some tests are for.
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// The `q` parameter of a query string, percent-decoded.
fn statement_of(query: &str) -> String {
    let raw = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("q="))
        .unwrap_or("");
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |offset: usize| {
            bytes
                .get(at + offset)
                .and_then(|&byte| (byte as char).to_digit(16))
        };
        match bytes[at] {
            b'+' => decoded.push(b' '),
            b'%' if hex(1).is_some() && hex(2).is_some() => {
                decoded.push((hex(1).unwrap_or(0) * 16 + hex(2).unwrap_or(0)) as u8);
                at += 2;
            }
            byte => decoded.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_statement_is_percent_decoded() {
        assert_eq!(
            statement_of("q=SELECT+%22a%22%2C+1%25&max_rows=5"),
            "SELECT \"a\", 1%"
        );
        assert_eq!(statement_of("max_rows=5"), "");
    }
}
