//! Holds a setup run at a step, so a test can look at it there.
#![allow(clippy::unwrap_used, dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use construct_agent::dashboard_setup::Step;

/// Holds a run at a step until it is let go.
#[derive(Default)]
pub struct Hold {
    at: Mutex<Option<Step>>,
    open: AtomicBool,
    reached: Mutex<Vec<Step>>,
}

impl Hold {
    pub fn at(&self, step: Step) {
        *self.at.lock().unwrap() = Some(step);
        self.open.store(false, Ordering::SeqCst);
    }

    pub fn release(&self) {
        self.open.store(true, Ordering::SeqCst);
    }

    pub fn reached(&self, step: Step) -> bool {
        self.reached.lock().unwrap().contains(&step)
    }

    pub fn pass(&self, step: Step) {
        self.reached.lock().unwrap().push(step);
        while *self.at.lock().unwrap() == Some(step) && !self.open.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
