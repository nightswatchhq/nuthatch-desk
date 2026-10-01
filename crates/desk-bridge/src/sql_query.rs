//! `SqlQuery`: one statement, run on the nest's `/sql`.

/// The bridge for [`SqlQuery`](qobject::SqlQuery).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// `QString` from `cxx_qt_lib`.
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// One statement, run on the nest's `/sql`.
        ///
        /// - **Owner:** QML. The Rust struct lives inside the C++ object and drops with it.
        /// - **Thread:** the GUI thread, for every property, signal and invokable.
        /// - **Crosses the boundary:** `QString`, integers and a boolean, by value. The result
        ///   itself does not cross here: it is published under `resultKey` for a `ResultModel`.
        /// - **Worker:** one short-lived thread per `run`, holding the statement, the URL and this
        ///   object's queue handle. It is never joined.
        ///
        /// `cancel` abandons the answer rather than the request: HTTP has no way to call a
        /// statement back, so the nest finishes it and the reply is dropped on arrival.
        #[qobject]
        #[qml_element]
        #[qproperty(QString, url)]
        #[qproperty(bool, running, READ, NOTIFY = changed)]
        #[qproperty(QString, error, READ, NOTIFY = changed)]
        #[qproperty(QString, notice, READ, NOTIFY = changed)]
        #[qproperty(i32, elapsed_ms, cxx_name = "elapsedMs", READ, NOTIFY = changed)]
        #[qproperty(i32, row_count, cxx_name = "rowCount", READ, NOTIFY = changed)]
        #[qproperty(i64, result_key, cxx_name = "resultKey", READ, NOTIFY = changed)]
        type SqlQuery = super::SqlQueryRust;

        /// Some read-only property changed.
        #[qsignal]
        fn changed(self: Pin<&mut SqlQuery>);

        /// A statement was answered. The result is under `resultKey`.
        #[qsignal]
        fn finished(self: Pin<&mut SqlQuery>);

        /// A statement was not answered. `message` is the nest's own wording where it gave one.
        #[qsignal]
        fn failed(self: Pin<&mut SqlQuery>, message: &QString);

        /// Runs `sql` against the nest at `url`, in place of any statement still running.
        #[qinvokable]
        fn run(self: Pin<&mut SqlQuery>, sql: &QString);

        /// Stops waiting for the statement that is running.
        #[qinvokable]
        fn cancel(self: Pin<&mut SqlQuery>);
    }

    impl cxx_qt::Threading for SqlQuery {}
}

use std::{
    pin::Pin,
    sync::OnceLock,
    time::{Duration, Instant},
};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use desk_core::{grid::Grid, results};
use nest_client::{Client, Error, Limits, config::check_url, sql::Answer};

use crate::guard::{contained, contained_or, to_i32};

/// How long a statement may run before the client stops waiting.
const SQL_TIMEOUT: Duration = Duration::from_secs(60);

fn client() -> Result<&'static Client, Error> {
    static CLIENT: OnceLock<Result<Client, Error>> = OnceLock::new();
    CLIENT
        .get_or_init(|| Client::new(SQL_TIMEOUT, Limits::default()))
        .as_ref()
        .map_err(Clone::clone)
}

/// The Rust state behind [`qobject::SqlQuery`].
#[derive(Default)]
pub struct SqlQueryRust {
    url: QString,
    running: bool,
    error: QString,
    notice: QString,
    elapsed_ms: i32,
    row_count: i32,
    result_key: i64,

    /// Counts runs and cancels, so an answer to anything but the latest run is dropped.
    generation: u64,
}

impl Drop for SqlQueryRust {
    fn drop(&mut self) {
        results::retire(self.result_key);
    }
}

impl qobject::SqlQuery {
    fn run(mut self: Pin<&mut Self>, sql: &QString) {
        contained("SqlQuery.run", || {
            let generation = self.generation + 1;
            self.as_mut().rust_mut().generation = generation;

            let statement = sql.to_string();
            if statement.trim().is_empty() {
                return self.fail("nothing to run");
            }
            let endpoint = match check_url(&self.url.to_string()) {
                Ok(endpoint) => endpoint,
                Err(problem) => return self.fail(&problem.to_string()),
            };
            let client = match client() {
                Ok(client) => client,
                Err(error) => return self.fail(&error.to_string()),
            };

            let thread = self.qt_thread();
            let started = Instant::now();
            let worker = std::thread::Builder::new()
                .name("sql".into())
                .spawn(move || {
                    // A panic in the request costs this statement, not the process.
                    let answer = contained_or("a statement", Err(Error::Failed), || {
                        client.sql(&endpoint.url, &statement)
                    });
                    let elapsed = started.elapsed();
                    // Refused once the QObject is destroyed, and then there is nobody to tell.
                    let _ = thread.queue(move |query| {
                        contained("SqlQuery's answer", || {
                            query.answer(generation, answer, elapsed);
                        });
                    });
                });
            if worker.is_err() {
                return self.fail("could not start a thread for the statement");
            }
            {
                let mut rust = self.as_mut().rust_mut();
                rust.running = true;
                rust.error = QString::default();
            }
            self.changed();
        });
    }

    fn cancel(mut self: Pin<&mut Self>) {
        contained("SqlQuery.cancel", || {
            if !self.running {
                return;
            }
            {
                let mut rust = self.as_mut().rust_mut();
                rust.generation += 1;
                rust.running = false;
            }
            self.changed();
        });
    }

    /// Runs on the GUI thread, queued by the statement's worker.
    fn answer(mut self: Pin<&mut Self>, generation: u64, answer: Result<Answer, Error>, elapsed: Duration) {
        if generation != self.generation {
            return;
        }
        self.as_mut().rust_mut().elapsed_ms = i32::try_from(elapsed.as_millis()).unwrap_or(i32::MAX);
        match answer {
            Ok(answer) => {
                let grid = Grid::from(answer);
                let (rows, notice) = (grid.rows.len(), grid.notice());
                let retired = self.result_key;
                {
                    let mut rust = self.as_mut().rust_mut();
                    rust.running = false;
                    rust.row_count = to_i32(rows);
                    rust.notice = QString::from(&notice);
                    rust.result_key = results::publish(grid);
                }
                results::retire(retired);
                self.as_mut().changed();
                self.finished();
            }
            Err(error) => self.fail(&error.to_string()),
        }
    }

    fn fail(mut self: Pin<&mut Self>, reason: &str) {
        let message = QString::from(reason);
        {
            let mut rust = self.as_mut().rust_mut();
            rust.running = false;
            rust.error = message.clone();
        }
        self.as_mut().changed();
        self.failed(&message);
    }
}
