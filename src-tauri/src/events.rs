// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Every event the core sends to the webview: one name, one payload type, one
//! function each. Nothing else calls `emit`.
//!
//! The webview side is `src/lib/events.ts` (`EventPayloads`), and a test keeps
//! the two lists of names equal. Before this module each sender spelled the name
//! itself, and `playtime-updated` went out as `null`, `""` or a game id depending
//! on who sent it.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const WINDOW_VISIBILITY: &str = "window-visibility";
pub const USER_DATA_IMPORTED: &str = "user-data-imported";
pub const UPDATE_CHANNEL_CHANGED: &str = "update-channel-changed";
pub const SETTINGS_UPDATED: &str = "settings-updated";
pub const OPEN_SPOTLIGHT: &str = "open-spotlight";
pub const OVERLAY_HEALTH: &str = "overlay-health";
pub const PLAYTIME_UPDATED: &str = "playtime-updated";

/// Every event name, for the test that compares them with the webview's list.
#[cfg(test)]
const ALL: [&str; 7] = [
    WINDOW_VISIBILITY,
    USER_DATA_IMPORTED,
    UPDATE_CHANNEL_CHANGED,
    SETTINGS_UPDATED,
    OPEN_SPOTLIGHT,
    OVERLAY_HEALTH,
    PLAYTIME_UPDATED,
];

fn send<P: Serialize + Clone>(app: &AppHandle, name: &str, payload: P) {
    if let Err(e) = app.emit(name, payload) {
        log::warn!("could not emit {name}: {e}");
    }
}

/// The main window was shown (`true`) or hidden to the tray (`false`).
pub fn window_visibility(app: &AppHandle, visible: bool) {
    send(app, WINDOW_VISIBILITY, visible);
}

/// A backup was restored: the library and categories must be re-read.
pub fn user_data_imported(app: &AppHandle) {
    send(app, USER_DATA_IMPORTED, ());
}

/// The update channel changed: check for an update on the new one.
pub fn update_channel_changed(app: &AppHandle) {
    send(app, UPDATE_CHANNEL_CHANGED, ());
}

/// Settings were written: every window re-reads them.
pub fn settings_updated(app: &AppHandle) {
    send(app, SETTINGS_UPDATED, ());
}

/// The spotlight shortcut was pressed.
pub fn open_spotlight(app: &AppHandle) {
    send(app, OPEN_SPOTLIGHT, ());
}

/// Live HUD composition health: 0 unknown, 1 on a hardware plane, 2 composed.
pub fn overlay_health(app: &AppHandle, health: u8) {
    send(app, OVERLAY_HEALTH, health);
}

/// Play stats changed: `Some(id)` for one game, `None` when any game's may have
/// (a restore, a new alias map, crash recovery).
pub fn playtime_updated(app: &AppHandle, id: Option<&str>) {
    send(app, PLAYTIME_UPDATED, id);
}

#[cfg(test)]
mod tests {
    use super::ALL;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn manifest_dir() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn the_webview_listens_for_exactly_the_events_the_core_sends() {
        let ts = std::fs::read_to_string(manifest_dir().join("../src/lib/events.ts"))
            .expect("src/lib/events.ts");
        let start = ts.find("export interface EventPayloads").expect("EventPayloads");
        let end = start + ts[start..].find('}').expect("end of EventPayloads");
        let webview: BTreeSet<&str> = ts[start..end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix('\''))
            .filter_map(|l| l.split('\'').next())
            .collect();
        let core: BTreeSet<&str> = ALL.into_iter().collect();
        assert_eq!(core.len(), ALL.len(), "duplicate event name");
        assert_eq!(webview, core);
    }

    #[test]
    fn nothing_but_this_module_emits() {
        // A raw emit is how a second payload shape for the same event appears.
        let needle = concat!(".emit", "(");
        let mut dirs = vec![manifest_dir().join("src")];
        let mut checked = 0;
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("source dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("events.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("source file");
                assert!(!text.contains(needle), "{} emits outside events.rs", path.display());
                checked += 1;
            }
        }
        // The walk must have seen the command modules, not just the top level.
        assert!(checked > 40, "only {checked} source files checked");
    }
}
