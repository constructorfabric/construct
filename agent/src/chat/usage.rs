//! The size of what an answer sent to the provider and received from it.

use std::sync::atomic::{AtomicU64, Ordering};

use super::wire::PendingCall;

/// The bytes of one answer: the request bodies sent, and the text and the tool calls received,
/// the sub-agents' included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub sent: u64,
    pub received: u64,
}

/// The running count of one answer.
#[derive(Default)]
pub(super) struct Counter {
    sent: AtomicU64,
    received: AtomicU64,
}

impl Counter {
    pub(super) fn sent(&self, bytes: usize) {
        self.sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(super) fn received(&self, bytes: usize) {
        self.received.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// The calls a turn asked for: their names and arguments.
    pub(super) fn calls(&self, calls: &[PendingCall]) {
        self.received(
            calls
                .iter()
                .map(|call| call.name.len() + call.arguments.len())
                .sum(),
        );
    }

    pub(super) fn total(&self) -> Usage {
        Usage {
            sent: self.sent.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
        }
    }
}
