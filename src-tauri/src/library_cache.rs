// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! `library_cache.json`: the last library `get_library` computed.
//!
//! It is a contract, not a cache. The frontend paints it at startup
//! (`cached_library`), commands resolve the ids they receive through it
//! (`resolve_game`), and the playtime watcher matches running processes against
//! it, re-reading it whenever its modification time changes.
//!
//! This module is the file's only owner: its name, how it is written and how it
//! is read. The watcher needs only part of each entry and reads its own view of
//! it (`read_as`); a test next to that view pins that it reads what `write`
//! writes. The name used to be declared twice, once per side, with nothing
//! tying the two shapes together (2026-09-27 audit, K18).

use std::time::SystemTime;

use serde::de::DeserializeOwned;
use tauri::AppHandle;

use crate::jsonstore;
use crate::models::Game;

/// File name inside the app data directory.
pub(crate) const FILE: &str = "library_cache.json";

/// Replace the snapshot with `games`. A failure is logged and otherwise ignored:
/// the library in memory is still right, and the next scan writes it again.
pub(crate) fn write(app: &AppHandle, games: &[Game]) {
    if let Err(e) = jsonstore::save(app, FILE, &games) {
        log::warn!("could not write {FILE}: {e}");
    }
}

/// The last computed library (empty if never scanned or unreadable).
pub(crate) fn read(app: &AppHandle) -> Vec<Game> {
    read_as(app)
}

/// The snapshot read as a partial view of each entry, for readers that need
/// only some fields (empty if never scanned or unreadable).
pub(crate) fn read_as<E: DeserializeOwned>(app: &AppHandle) -> Vec<E> {
    jsonstore::load_or_default(app, FILE)
}

/// When the snapshot was last written, so a reader can re-read it only after a
/// scan instead of on a timer.
pub(crate) fn modified(app: &AppHandle) -> Option<SystemTime> {
    let path = jsonstore::path(app, FILE).ok()?;
    std::fs::metadata(path).ok()?.modified().ok()
}
