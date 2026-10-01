//! Rule 8: no panic crosses the boundary.
//!
//! A Rust panic that unwinds into a C++ frame aborts the process. Every invokable, every property
//! setter and every closure queued onto the GUI thread runs its body through one of these, so a
//! bug in this crate costs one update rather than the window.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `body`. A panic in it is reported on stderr and goes no further.
pub(crate) fn contained(what: &str, body: impl FnOnce()) {
    contained_or(what, (), body);
}

/// Runs `body` for its value, or gives `fallback` if it panics.
pub(crate) fn contained_or<T>(what: &str, fallback: T, body: impl FnOnce() -> T) -> T {
    // Unwind safety: the state a panic leaves behind is a QObject's plain Rust struct, which is
    // only ever read back into properties. A half-applied update shows stale figures at worst.
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| {
        eprintln!("nuthatch-desk: a panic in {what} was contained");
        fallback
    })
}

/// A count or height as a QML number. QML numbers are doubles, which hold every integer up to
/// 2^53, and a block height is nowhere near it.
pub(crate) fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// A row or column count as Qt wants it.
pub(crate) fn to_i32(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// A row or column index from Qt, when it is one.
pub(crate) fn to_index(value: i32) -> Option<usize> {
    usize::try_from(value).ok()
}
