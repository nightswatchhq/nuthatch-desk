//! A test hook, compiled only under the `lifetime-probe` feature.
//!
//! Rule 4 says a queue call made after its QObject is destroyed must fail cleanly. In the shipped
//! client that call is almost never made, because destroying the object takes its sink out of the
//! poller first. To make it on purpose, a test asks for the next subscription to be leaked: the
//! sink then outlives the object, the poller delivers to it, and the queue has to refuse.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static LEAK_NEXT: AtomicBool = AtomicBool::new(false);
static REFUSED: AtomicUsize = AtomicUsize::new(0);

/// Leaks the subscription made by the next `NestStatus.connectTo`.
pub fn leak_next_subscription() {
    LEAK_NEXT.store(true, Ordering::SeqCst);
}

/// How many deliveries a destroyed `NestStatus` has refused.
pub fn refused() -> usize {
    REFUSED.load(Ordering::SeqCst)
}

pub(crate) fn take_leak() -> bool {
    LEAK_NEXT.swap(false, Ordering::SeqCst)
}

pub(crate) fn count_refusal() {
    REFUSED.fetch_add(1, Ordering::SeqCst);
}
