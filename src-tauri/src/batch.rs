// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Bounded, order-preserving fan-out for batch commands.
//!
//! A batch command answers a list of questions in one IPC round trip. The answers
//! go back **in the order asked and one per question**, because the frontend pairs
//! them by index; a slot nobody filled stays `None` so the caller decides what
//! "no answer" means instead of getting a shorter list.
//!
//! Threads are scoped and owned by the call: nothing outlives it and nothing is
//! shared between two batches.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// Most questions one call may carry. The frontend sends far smaller chunks; this
/// is the bound on what a single message from the webview can make the core do.
pub const MAX_ITEMS: usize = 64;

/// Reject an oversized batch before any work starts.
pub fn check_len(len: usize) -> Result<(), String> {
    if len > MAX_ITEMS {
        return Err(format!("too many items in one batch: {len} (limit {MAX_ITEMS})"));
    }
    Ok(())
}

/// Run `work` over `items` on at most `workers` threads (the calling thread is one
/// of them) and return the results in input order.
///
/// A worker that could not be spawned only costs parallelism: the calling thread
/// drains the queue by itself.
pub fn map_ordered<T, R, F>(items: &[T], workers: usize, work: F) -> Vec<Option<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let next = AtomicUsize::new(0);
    let drain = || {
        let mut done = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(item) = items.get(index) else {
                break;
            };
            done.push((index, work(item)));
        }
        done
    };

    let extra = workers.clamp(1, items.len().max(1)) - 1;
    let answered = thread::scope(|scope| {
        let handles: Vec<_> = (0..extra)
            .filter_map(|n| {
                thread::Builder::new()
                    .name(format!("batch-{n}"))
                    .spawn_scoped(scope, drain)
                    .map_err(|e| log::warn!("batch worker not started: {e}"))
                    .ok()
            })
            .collect();
        let mut all = drain();
        for handle in handles {
            all.extend(handle.join().unwrap_or_default());
        }
        all
    });

    let mut out: Vec<Option<R>> = items.iter().map(|_| None).collect();
    for (index, result) in answered {
        if let Some(slot) = out.get_mut(index) {
            *slot = Some(result);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn answers_come_back_in_the_order_asked() {
        // Later items finish first; the output order must not follow completion.
        let items: Vec<u64> = (0..12).collect();
        let out = map_ordered(&items, 4, |n| {
            thread::sleep(Duration::from_millis((12 - n) * 2));
            n * 10
        });
        let expected: Vec<Option<u64>> = items.iter().map(|n| Some(n * 10)).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn every_item_is_worked_exactly_once() {
        let items: Vec<usize> = (0..50).collect();
        let seen = Mutex::new(Vec::new());
        let out = map_ordered(&items, 4, |n| {
            seen.lock().unwrap_or_else(|e| e.into_inner()).push(*n);
        });
        assert_eq!(out.len(), 50);
        assert!(out.iter().all(Option::is_some));
        let seen = seen.into_inner().unwrap_or_else(|e| e.into_inner());
        assert_eq!(seen.len(), 50);
        assert_eq!(seen.iter().collect::<HashSet<_>>().len(), 50);
    }

    #[test]
    fn the_work_is_spread_but_never_wider_than_asked() {
        let items: Vec<u8> = vec![0; 16];
        let threads = Mutex::new(HashSet::new());
        map_ordered(&items, 3, |_| {
            threads
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(thread::current().id());
            thread::sleep(Duration::from_millis(5));
        });
        let used = threads.into_inner().unwrap_or_else(|e| e.into_inner()).len();
        assert!((1..=3).contains(&used), "used {used} threads");
    }

    #[test]
    fn degenerate_inputs_are_fine() {
        let none: Vec<u8> = Vec::new();
        assert!(map_ordered(&none, 4, |n| *n).is_empty());
        assert_eq!(map_ordered(&[7u8], 0, |n| *n), vec![Some(7)]);
    }

    #[test]
    fn an_oversized_batch_is_refused() {
        assert!(check_len(0).is_ok());
        assert!(check_len(MAX_ITEMS).is_ok());
        let err = check_len(MAX_ITEMS + 1).unwrap_err();
        assert!(err.contains("limit 64"), "{err}");
    }
}
