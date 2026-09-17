// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Opt-in timing spans for the performance work (`docs/perf/`).
//!
//! Enabled only when `METEOR_PERF` is set in the environment, so a shipped build
//! pays one relaxed atomic load per span and nothing else. Output goes to stderr
//! as `perf <name> <ms>` lines, which `docs/perf/capture.ps1` parses.
//!
//! Usage:
//! ```ignore
//! let _span = crate::perf::Span::new("get_library");
//! ```

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

/// 0 = not checked yet, 1 = disabled, 2 = enabled.
static STATE: AtomicU8 = AtomicU8::new(0);

/// Whether `METEOR_PERF` was set when this process started.
pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = std::env::var_os("METEOR_PERF").is_some();
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// Times its scope and prints `perf <name> <ms>` on drop. No-op when disabled.
pub struct Span {
    name: &'static str,
    start: Option<Instant>,
}

impl Span {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: enabled().then(Instant::now),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            eprintln!("perf {} {:.1}", self.name, start.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
