//! Where a finished result waits for the model that will show it.
//!
//! Two QObjects cannot hold each other, so a result crosses from the object that fetched it to the
//! model that displays it by value: the fetcher publishes the grid here and exposes the key as a
//! property, and QML binds the model's key to it. Everything here is called on the GUI thread, and
//! the lock is only there because a `static` needs one.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicI64, Ordering},
    },
};

use crate::grid::Grid;

static NEXT: AtomicI64 = AtomicI64::new(1);

fn store() -> MutexGuard<'static, HashMap<i64, Arc<Grid>>> {
    static STORE: OnceLock<Mutex<HashMap<i64, Arc<Grid>>>> = OnceLock::new();
    STORE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Publishes `grid` and returns its key. A key is never zero and never reused.
pub fn publish(grid: Grid) -> i64 {
    let key = NEXT.fetch_add(1, Ordering::Relaxed);
    store().insert(key, Arc::new(grid));
    key
}

/// The grid published under `key`, if it has not been retired.
pub fn get(key: i64) -> Option<Arc<Grid>> {
    store().get(&key).cloned()
}

/// Forgets `key`. A model already showing the grid keeps its own reference.
pub fn retire(key: i64) {
    store().remove(&key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_is_found_until_it_is_retired() {
        let key = publish(Grid::default());
        assert_ne!(key, 0);
        let held = get(key).unwrap();
        retire(key);
        assert!(get(key).is_none());
        assert!(held.columns.is_empty());
        assert_ne!(publish(Grid::default()), key);
    }
}
