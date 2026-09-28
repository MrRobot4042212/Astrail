// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! The main window and the in-game overlay window: showing, hiding to the
//! tray, the idle webview trim, and the on-demand overlay settings screen.

use crate::*;

/// Seconds the main window must stay hidden before we ask WebView2 to trim its
/// memory. Short enough to matter when Astrail lives in the tray, long enough not
/// to fire on a hide/show bounce.
#[cfg(windows)]
pub(crate) const WEBVIEW_TRIM_DELAY_SECS: u64 = 10;

/// Ask WebView2 to drop what it can (`LOW`) or go back to normal.
///
/// Closing the window only hides it — the whole Chromium process tree stays
/// resident so the playtime/Discord watchers keep running. `LOW` lets the engine
/// release caches and decoded images while nobody is looking, and unlike
/// `TrySuspend` it has no lifecycle semantics that could break the page state.
#[cfg(windows)]
pub(crate) fn set_webview_memory_low(app: &AppHandle, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use tauri::webview::PlatformWebview;
    use windows::core::Interface;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.with_webview(move |webview: PlatformWebview| {
        // SAFETY: runs on the UI thread that owns the controller (Tauri
        // guarantees this for `with_webview`), and only reads/sets a setting.
        unsafe {
            let Ok(core) = webview.controller().CoreWebView2() else {
                return;
            };
            // ICoreWebView2_19 needs a recent runtime; older ones just skip it.
            let Ok(api) = core.cast::<ICoreWebView2_19>() else {
                return;
            };
            let _ = api.SetMemoryUsageTargetLevel(if low {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            });
        }
    });
}

/// Tell the frontend whether its window is visible.
///
/// The webview keeps running while hidden to the tray, and `document.hidden` is
/// not reliable when only the HWND is hidden, so visibility is published from
/// here: the library hook uses it to stop its periodic rescan while nobody can
/// see the result.
pub(crate) fn emit_visibility(app: &AppHandle, visible: bool) {
    events::window_visibility(app, visible);
}

/// Reveal the main window once the frontend has painted.
///
/// The window is created with `"visible": false` so the user never sees an empty
/// white rectangle while the webview boots (it is also `maximized` + `center`,
/// which made that flash very visible).
///
/// A start from the autostart `Run` key (`--minimized`) skips this first reveal
/// and stays in the tray, once the user is past onboarding and closing to the
/// tray is on (otherwise the tray is not where Astrail lives, and staying hidden
/// would look like a failed start).
// NOT `async`: shows a window, which belongs on the main thread.
#[tauri::command]
pub(crate) fn show_main_window(app: AppHandle) {
    if START_HIDDEN.swap(false, std::sync::atomic::Ordering::Relaxed) {
        let (tray, setup) = app
            .try_state::<std::sync::Mutex<AppSettings>>()
            .map(|s| {
                let s = lock_settings(&s);
                (s.minimize_to_tray, s.setup_completed)
            })
            .unwrap_or((false, false));
        if stays_in_tray(true, tray, setup) {
            log::info!("started from autostart: staying in the tray");
            emit_visibility(&app, false);
            #[cfg(windows)]
            schedule_webview_trim(app);
            return;
        }
    }
    show_main(&app);
}

/// Set once in `run()` when the process was started with
/// `autostart::MINIMIZED_ARG`; consumed by the first `show_main_window`.
pub(crate) static START_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a start keeps the main window hidden: only a logon start, only after
/// onboarding, and only when closing the window hides to the tray.
pub(crate) fn stays_in_tray(started_minimized: bool, minimize_to_tray: bool, setup_completed: bool) -> bool {
    started_minimized && minimize_to_tray && setup_completed
}

/// Main-window visibility, for a listener that registered after the first
/// `window-visibility` event was already sent.
// NOT `async`: a window getter, which belongs on the main thread.
#[tauri::command]
pub(crate) fn main_window_visible(app: AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(true)
}

/// Trim WebView2's memory once the main window has been hidden for a while,
/// and only if it is still hidden by then.
#[cfg(windows)]
pub(crate) fn schedule_webview_trim(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(WEBVIEW_TRIM_DELAY_SECS));
        let hidden = app
            .get_webview_window("main")
            .and_then(|w| w.is_visible().ok())
            .map(|visible| !visible)
            .unwrap_or(false);
        if hidden {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                set_webview_memory_low(&handle, true);
            });
        }
    });
}

/// Bring the main window to the front (used by the tray and Spotlight).
pub(crate) fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        #[cfg(windows)]
        set_webview_memory_low(app, false);
        emit_visibility(app, true);
    }
}

/// Cover the monitor the game is on with the in-game settings window.
///
/// The foreground window is still the game when the hotkey fires (the settings
/// window is created unfocused). It used to take `primary_monitor()`, so on a
/// multi-monitor setup the settings screen opened on another display than the game.
pub(crate) fn cover_game_monitor(w: &tauri::WebviewWindow) {
    use tauri::{PhysicalPosition, PhysicalSize};
    let mon = metrics::monitor_geometry(overlay::foreground());
    let _ = w.set_size(PhysicalSize::new(mon.width.max(1) as u32, mon.height.max(1) as u32));
    let _ = w.set_position(PhysicalPosition::new(mon.left, mon.top));
}

/// Apply the overlay config live: update the sampler and snapshot the full config
/// for the native HUD renderer. The native HUD (drawn by the sampler) reflects this
/// on its next tick; when `enabled` is false the sampler hides it.
pub(crate) fn apply_overlay_settings(_app: &AppHandle, settings: &AppSettings) {
    metrics::configure(&settings.overlay);
    metrics::set_gpu(settings.overlay.gpu.clone());
    // Snapshot the full overlay config (colors, position, font size, opacity, which
    // metrics) so the native HUD renderer reads it each tick.
    metrics::set_render_cfg(settings.overlay.clone());
}

/// Toggle the overlay on/off (the global hotkey). Persists and applies live.
pub(crate) fn toggle_overlay(app: &AppHandle) {
    if let Some(state) = app.try_state::<std::sync::Mutex<AppSettings>>() {
        // The change and its snapshot number under one lock, so a concurrent patch
        // cannot interleave; the write happens off this (main) thread and outside
        // the lock (see `storage::persist_settings`).
        let (s, seq) = {
            let mut current = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            current.overlay.enabled = !current.overlay.enabled;
            (current.clone(), storage::settings_seq())
        };
        let (handle, snapshot) = (app.clone(), s.clone());
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = storage::persist_settings(&handle, &snapshot, seq) {
                log::warn!("could not save the overlay toggle: {e}");
            }
        });
        apply_overlay_settings(app, &s);
        // An open settings screen shows the toggle too.
        events::settings_updated(app);
    }
}

/// Create the in-game overlay *settings* WebView window on demand. It is **not**
/// created at startup and is destroyed when the settings screen closes, so a full
/// WebView2/Chromium stack (browser + renderer + GPU process) never sits resident
/// during gameplay — the HUD itself is drawn by the native window, so this window's
/// only purpose is the settings screen. Returns the existing window if already open.
pub(crate) fn ensure_overlay_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window("overlay") {
        return Some(w);
    }
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    // Its own document, not `index.html`: pointing both windows at the launcher's
    // entry made the overlay download and evaluate the entire launcher bundle
    // (~854 KB of eager JS, the grid and both catalogs included) to draw one
    // settings panel while a game is running. Next code-splits per route, so this
    // loads only the overlay tree. No `.html`: the dev server only knows the route
    // `/overlay`, and the bundled app resolves `overlay` to `overlay.html` itself.
    WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay".into()))
        .title("Astrail Overlay")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()
        .ok()
}

/// Toggle the in-game overlay settings screen (the overlay-settings hotkey). Creates
/// the WebView window on open and destroys it on close, so nothing Chromium-related
/// stays resident while playing.
pub(crate) fn toggle_overlay_settings(app: &AppHandle) {
    if metrics::settings_open() {
        let _ = set_overlay_interactive(app.clone(), false);
    } else {
        let _ = set_overlay_interactive(app.clone(), true);
    }
}

/// Open or close the in-game overlay *settings* screen. On open it **creates** the
/// overlay WebView (covering the monitor, taking input) and the native HUD hides (the
/// sampler gates on `set_settings_open`). On close it **destroys** the window, freeing
/// the WebView2 processes — zero Chromium overhead during gameplay.
// NOT `async`: creates/destroys a WebviewWindow and moves focus, which belongs
// on the main thread. It does no I/O.
#[tauri::command]
pub(crate) fn set_overlay_interactive(app: AppHandle, interactive: bool) -> CmdResult<()> {
    metrics::set_settings_open(interactive);
    if interactive {
        let Some(w) = ensure_overlay_window(&app) else {
            return Err("Could not create the overlay window".into());
        };
        w.set_ignore_cursor_events(false).map_err(|e| e.to_string())?;
        cover_game_monitor(&w);
        let _ = w.show();
        let _ = w.set_focus();
    } else if let Some(w) = app.get_webview_window("overlay") {
        // Destroy (not just hide) so the WebView2 processes are released.
        let _ = w.close();
    }
    Ok(())
}
