// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! The one running sidecar child, shared by its controller thread and the exit
//! path (`presentmon`, `cputemp`).
//!
//! A controller takes the child out of the slot before stopping it, so the slot
//! is not locked for the whole graceful-stop wait. That left the exit path a
//! window: it found the slot empty and returned while the controller was still
//! stopping the child, the process exited, and the kill-on-close job terminated
//! the sidecar before PresentMon's ETW session or cputemp's kernel driver was
//! released (2026-09-27 audit, X-I1). Every step that changes which child runs
//! (stop, spawn, reap) therefore holds a second lock, taken with [`Slot::step`],
//! and [`Slot::shutdown`] takes it first: it waits for a stop in flight, and a
//! spawn cannot land behind it.
//!
//! Lock order: the step lock, then the child lock, never the reverse.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub struct Slot<T> {
    child: Mutex<Option<T>>,
    step: Mutex<()>,
}

impl<T> Slot<T> {
    pub const fn new() -> Self {
        Self {
            child: Mutex::new(None),
            step: Mutex::new(()),
        }
    }

    /// The child itself. Hold it briefly: never across a stop or a spawn.
    pub fn child(&self) -> MutexGuard<'_, Option<T>> {
        self.child.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Serialize one controller step that stops, spawns or reaps the child. Take it
    /// before [`Slot::child`] and keep it until the step is complete.
    pub fn step(&self) -> MutexGuard<'_, ()> {
        self.step.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The exit path: wait for any step in flight, then stop the child that is
    /// left, if any. Returns whether there was one.
    pub fn shutdown(&self, stop: impl FnOnce(T)) -> bool {
        let _step = self.step();
        let child = self.child().take();
        match child {
            Some(child) => {
                stop(child);
                true
            }
            None => false,
        }
    }
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn shutdown_waits_for_a_stop_already_in_flight() {
        // Regression (X-I1): the controller had taken the child out and was inside
        // its graceful stop; `shutdown` saw an empty slot and returned at once, so
        // the process exited mid-stop.
        static SLOT: Slot<u32> = Slot::new();
        static STOPPED: AtomicBool = AtomicBool::new(false);
        *SLOT.child() = Some(7);

        let (taken, is_taken) = mpsc::channel();
        let controller = std::thread::spawn(move || {
            let _step = SLOT.step();
            let child = SLOT.child().take();
            taken.send(()).expect("test channel");
            std::thread::sleep(Duration::from_millis(150));
            STOPPED.store(child.is_some(), Ordering::SeqCst);
        });

        is_taken.recv().expect("test channel");
        let had_child = SLOT.shutdown(|_| panic!("the controller owns this child"));
        assert!(!had_child);
        assert!(STOPPED.load(Ordering::SeqCst), "shutdown returned before the stop ended");
        controller.join().expect("controller thread");
    }

    #[test]
    fn shutdown_stops_the_child_that_is_left() {
        static SLOT: Slot<u32> = Slot::new();
        *SLOT.child() = Some(3);
        let mut stopped = None;
        assert!(SLOT.shutdown(|c| stopped = Some(c)));
        assert_eq!(stopped, Some(3));
        assert!(SLOT.child().is_none());
        assert!(!SLOT.shutdown(|_| unreachable!()));
    }
}
