// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Settings: reading, the strict patch the webview sends, what a change
//! re-applies (overlay, shortcuts, Discord, autostart), and the global shortcuts.

use crate::*;

/// Whether Astrail is set to launch on Windows login (the autostart `Run` key),
/// and whether this copy may turn it on (only an installed one may).
#[tauri::command(async)]
pub(crate) fn get_autostart() -> CmdResult<autostart::AutostartState> {
    Ok(autostart::AutostartState {
        enabled: autostart::is_enabled().map_err(|e| format!("Failed to read autostart: {e}"))?,
        available: autostart::available(),
    })
}

/// Autostart is the `Run` key only, elevated or not: Astrail never starts itself
/// elevated at logon (see `elevation::remove_legacy_logon_task`). Both branches
/// are idempotent — enabling rewrites the quoted path, disabling ignores a
/// missing value — so no state check is needed first.
#[tauri::command(async)]
pub(crate) fn set_autostart(enabled: bool) -> CmdResult<()> {
    let result = if enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    result.map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::Unsupported {
            ErrorCode::AutostartUnavailable
        } else {
            ErrorCode::Io
        };
        AppError::new(code, format!("Failed to update autostart: {e}"))
    })
}

/// Lock the settings, recovering from a poisoned mutex.
///
/// Every reader of the settings runs on the main thread (commands, the close
/// handler, the hotkey handler). With `panic = "abort"` a plain `unwrap()` there
/// turns one earlier panic under the lock into the whole app dying on the next
/// window close or key press; the data under the lock is a plain struct that is
/// never left half-written, so the poisoned value is safe to keep using.
pub(crate) fn lock_settings(state: &std::sync::Mutex<AppSettings>) -> std::sync::MutexGuard<'_, AppSettings> {
    state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The settings in memory, or the stored ones before the state is managed.
pub(crate) fn current_settings(app: &AppHandle) -> AppSettings {
    app.try_state::<std::sync::Mutex<AppSettings>>()
        .map(|s| lock_settings(&s).clone())
        .unwrap_or_else(|| storage::load_settings(app))
}

#[tauri::command]
pub(crate) fn get_app_settings(state: tauri::State<'_, std::sync::Mutex<AppSettings>>) -> CmdResult<AppSettings> {
    Ok(lock_settings(&state).clone())
}

/// Change only the settings named in `patch`, atomically.
///
/// There is deliberately no whole-struct setter. A window that reads the settings,
/// spreads its change over them and writes everything back loses a write made by
/// another window in between (the launcher and the in-game screen can both be
/// open). The merge happens here, under the settings lock, and only the parts
/// that changed are re-applied: a HUD color does not re-register the hotkeys.
#[tauri::command(async)]
pub(crate) fn patch_app_settings(
    app: AppHandle,
    state: tauri::State<'_, std::sync::Mutex<AppSettings>>,
    patch: serde_json::Value,
) -> CmdResult<()> {
    log::debug!("settings patch: {patch}");
    let (previous, next) = {
        let mut current = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = apply_settings_patch(&current, patch).map_err(AppError::with(ErrorCode::InvalidInput))?;
        storage::save_settings(&app, &next);
        (std::mem::replace(&mut *current, next.clone()), next)
    };
    settings_changed(&app, &previous, &next);
    Ok(())
}

/// Merge a partial settings object into `current`. Nested objects merge key by
/// key; any key the settings do not have is an error rather than silently ignored.
pub(crate) fn apply_settings_patch(current: &AppSettings, patch: serde_json::Value) -> Result<AppSettings, String> {
    fn merge(target: &mut serde_json::Value, patch: serde_json::Value, path: &str) -> Result<(), String> {
        let serde_json::Value::Object(fields) = patch else {
            return Err(format!("Settings patch at `{path}` must be an object"));
        };
        let serde_json::Value::Object(target) = target else {
            return Err(format!("Setting `{path}` is not an object"));
        };
        for (key, value) in fields {
            let at = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
            let slot = target.get_mut(&key).ok_or_else(|| format!("Unknown setting `{at}`"))?;
            if slot.is_object() && value.is_object() {
                merge(slot, value, &at)?;
            } else {
                *slot = value;
            }
        }
        Ok(())
    }
    /// The first leaf where `a` and `b` differ, as a dotted path.
    fn first_difference(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => a.iter().find_map(|(key, va)| {
                let at = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match b.get(key) {
                    Some(vb) => first_difference(va, vb, &at),
                    None => Some(at),
                }
            }),
            _ => (a != b).then(|| path.to_string()),
        }
    }
    let mut merged = serde_json::to_value(current).map_err(|e| e.to_string())?;
    merge(&mut merged, patch, "")?;
    let next: AppSettings =
        serde_json::from_value(merged.clone()).map_err(|e| format!("Invalid settings patch: {e}"))?;
    // Some fields are read leniently (an unknown value becomes the default, so a
    // hand-edited file still loads). From the webview that would turn a typo into
    // a silent change, so a value that does not come back unchanged is refused.
    let back = serde_json::to_value(&next).map_err(|e| e.to_string())?;
    if let Some(at) = first_difference(&merged, &back, "") {
        return Err(format!("Invalid value for setting `{at}`"));
    }
    Ok(next)
}

/// Re-apply what changed between two settings snapshots and notify every window.
pub(crate) fn settings_changed(app: &AppHandle, previous: &AppSettings, next: &AppSettings) {
    fn same<T: serde::Serialize>(a: &T, b: &T) -> bool {
        matches!((serde_json::to_value(a), serde_json::to_value(b)), (Ok(a), Ok(b)) if a == b)
    }
    if !same(&previous.overlay, &next.overlay) {
        apply_overlay_settings(app, next);
    }
    discord::set_enabled(next.discord_enabled);
    if previous.track_external_games != next.track_external_games {
        log::info!("external game tracking: {}", next.track_external_games);
        playtime::set_external_tracking(next.track_external_games);
    }
    if previous.update_channel != next.update_channel {
        log::info!("update channel: {:?} -> {:?}", previous.update_channel, next.update_channel);
        // The update prompt checks again right away instead of at the next start.
        events::update_channel_changed(app);
    }
    if previous.language != next.language {
        // Native menu: main thread only.
        let handle = app.clone();
        let language = next.language.clone();
        let _ = app.run_on_main_thread(move || tray::set_language(&handle, &language));
    }
    if !same(&previous.shortcuts, &next.shortcuts) {
        // A window-manager operation: keep it on the main thread.
        let handle = app.clone();
        let shortcuts = next.shortcuts.clone();
        let _ = app.run_on_main_thread(move || register_shortcuts(&handle, &shortcuts));
    }
    events::settings_updated(app);
}

/// Parse a stored combination into a global shortcut, refusing unsafe ones.
///
/// `RegisterHotKey` takes the key away from every application on the machine, games
/// included. The settings recorder used to store whatever key it saw, including the
/// Escape pressed to *cancel* recording, and this parser accepted it, so a bare
/// `Escape` became a system-wide hotkey. See `is_safe_global_shortcut`.
pub(crate) fn parse_shortcut(s: &str) -> Option<tauri_plugin_global_shortcut::Shortcut> {
    let mut s = s.to_string();
    if let Some(idx) = s.rfind('+') {
        let last = &s[idx+1..];
        if last.len() == 1 {
            let c = last.chars().next().unwrap();
            if c.is_ascii_uppercase() {
                s.replace_range(idx+1.., &format!("Key{}", c));
            } else if c.is_ascii_digit() {
                s.replace_range(idx+1.., &format!("Digit{}", c));
            }
        }
    }
    s.parse().ok().filter(is_safe_global_shortcut)
}

/// A global shortcut must carry Ctrl, Alt or Win. Shift alone is only accepted with
/// a function key: `Shift+A` would swallow capital letters everywhere, while
/// `Shift+F5` is not typing.
pub(crate) fn is_safe_global_shortcut(sc: &tauri_plugin_global_shortcut::Shortcut) -> bool {
    use tauri_plugin_global_shortcut::{Code, Modifiers};
    if sc.mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
        return true;
    }
    sc.mods.contains(Modifiers::SHIFT)
        && matches!(
            sc.key,
            Code::F1 | Code::F2 | Code::F3 | Code::F4 | Code::F5 | Code::F6 | Code::F7 | Code::F8
                | Code::F9 | Code::F10 | Code::F11 | Code::F12 | Code::F13 | Code::F14 | Code::F15
                | Code::F16 | Code::F17 | Code::F18 | Code::F19 | Code::F20 | Code::F21 | Code::F22
                | Code::F23 | Code::F24
        )
}

pub(crate) fn register_shortcuts(app: &AppHandle, shortcuts: &crate::models::ShortcutsSettings) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let _ = app.global_shortcut().unregister_all();

    // A global shortcut is a machine-wide claim on a key combination: `RegisterHotKey`
    // consumes the keystroke, so the foreground window (a game included) never sees
    // it. Registration also fails when another application already owns the combo —
    // which used to be swallowed, leaving a shortcut that silently never worked.
    for (what, combo) in [
        ("spotlight", &shortcuts.spotlight),
        ("overlay_toggle", &shortcuts.overlay_toggle),
        ("overlay_settings", &shortcuts.overlay_settings),
    ] {
        if combo.trim().is_empty() {
            continue;
        }
        let Some(parsed) = parse_shortcut(combo) else {
            log::warn!(
                "{what}: '{combo}' is not a valid combination or lacks a Ctrl/Alt/Win modifier; not registered"
            );
            continue;
        };
        if let Err(e) = app.global_shortcut().register(parsed) {
            log::warn!(
                "{what}: could not register '{combo}' (another application may own it): {e}"
            );
        }
    }
}

/// The saved Discord Rich Presence client id (empty = disabled).
#[tauri::command(async)]
pub(crate) fn get_discord_client_id(app: AppHandle) -> CmdResult<String> {
    Ok(storage::load_discord_client_id(&app))
}

/// Save the Discord client id and apply it live to the presence watcher.
#[tauri::command(async)]
pub(crate) fn set_discord_client_id(app: AppHandle, id: String) -> CmdResult<()> {
    storage::save_discord_client_id(&app, &id)?;
    discord::set_client_id(&id);
    Ok(())
}
