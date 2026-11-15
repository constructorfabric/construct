//! The one way the agent locks a mutex: a thread that panicked while holding it must not take
//! every other thread down with it.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `m`, taking the data of a poisoned mutex as it is.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
