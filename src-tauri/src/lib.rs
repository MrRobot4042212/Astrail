// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

mod about;
mod commands;
mod appicons;
mod applog;
mod apps_db;
mod art;
mod autostart;
mod backup;
mod batch;
mod cputemp;
#[cfg(windows)]
mod elevation;
mod error;
mod events;
mod discord;
mod battlenet;
mod ea;
mod epic;
mod fgwatch;
mod files;
mod fingerprint;
mod gog;
mod igdb;
mod jsonstore;
#[cfg(windows)]
mod jobobj;
mod launcher;
mod library;
mod metrics;
mod models;
mod perf;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod overlay_diag;
#[cfg(windows)]
mod overlay_dcomp;
#[cfg(windows)]
mod overlay_native;
mod playtime;
mod presentmon;
mod screenshots;
mod sessionperf;
mod sidecar_integrity;
mod sidecar_log;
mod steam;
mod steam_playtime;
mod storage;
#[cfg(windows)]
mod sysstat;
mod tray;
mod system;
mod ubisoft;
mod updates;
mod windows_apps;
mod xbox;

use models::{Category, Game, GameSource, AppSettings};
use std::time::{SystemTime, UNIX_EPOCH};
use error::{AppError, CmdResult, ErrorCode};
use tauri::{AppHandle, Manager};

use commands::{backup::*, library::*, settings::*, system::*, update::*, window::*};

/// Run a blocking command body on the runtime's **blocking** pool.
///
/// `#[tauri::command(async)]` on a synchronous function does not do this, despite
/// how it reads. The macro emits `let result = $path(args);` inside an
/// `async move` handed to `async_runtime::spawn` (`tauri-macros/command/wrapper.rs`
/// → `tauri/src/ipc/mod.rs`), so the whole blocking body occupies one of the
/// runtime's *worker* threads, and there are only `available_parallelism()` of
/// them. Enough concurrent cover lookups — each up to a 6 s connect plus an 8 s
/// read, times three name variants — leaves no worker free to dispatch anything
/// else, so `cached_library`, the settings and the launch response all queue
/// behind network calls. The blocking pool grows on demand instead.
///
/// Use it for anything that touches the network, walks the filesystem, reads the
/// registry, spawns a process or enumerates the system. Small in-memory work does
/// not need it.
async fn blocking<T, F>(f: F) -> CmdResult<T>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(result) => result.map_err(AppError::from),
        Err(e) => Err(AppError::new(ErrorCode::Internal, format!("background task failed: {e}"))),
    }
}

/// `blocking` for a body that already returns `AppError`, when failures inside it
/// need different codes.
async fn blocking_cmd<T, F>(f: F) -> CmdResult<T>
where
    F: FnOnce() -> CmdResult<T> + Send + 'static,
    T: Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(result) => result,
        Err(e) => Err(AppError::new(ErrorCode::Internal, format!("background task failed: {e}"))),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything else: a release build has no stderr, so without this a
    // failure in the hand-off below or in `build()` leaves nothing behind.
    let context = tauri::generate_context!();
    if let Some(dir) = applog::default_dir(&context.config().identifier) {
        applog::init(dir);
    }
    log::info!(
        "Astrail {} starting (elevated: {}, pid {})",
        env!("CARGO_PKG_VERSION"),
        is_elevated(),
        std::process::id()
    );

    #[cfg(windows)]
    elevation::await_previous_instance();
    START_HIDDEN.store(
        autostart::started_minimized(std::env::args()),
        std::sync::atomic::Ordering::Relaxed,
    );

    tauri::Builder::default()
        // First, so a second launch exits before it registers a tray icon or global
        // hotkeys, or starts a PresentMon whose `--stop_existing_session` would end
        // this instance's ETW session. The second launch just surfaces this window.
        // A second logon start (`--minimized`) while one is already running
        // stays out of the way.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !autostart::started_minimized(args) {
                show_main(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        // In-app auto-update (checks GitHub Releases) + relaunch after install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Closing the main window hides Astrail to the tray instead of quitting,
        // so the playtime/Discord/Spotlight watchers keep running. Real quit is
        // the tray's "Salir" item.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let minimize = window
                        .app_handle()
                        .try_state::<std::sync::Mutex<AppSettings>>()
                        .map(|s| lock_settings(&s).minimize_to_tray)
                        .unwrap_or(true);
                    
                    if minimize {
                        api.prevent_close();
                        let _ = window.hide();
                        let app = window.app_handle().clone();
                        emit_visibility(&app, false);
                        #[cfg(windows)]
                        schedule_webview_trim(app);
                    } else {
                        // Let it close, which exits the app.
                    }
                }
            }
        })
        .plugin(
            // Global Spotlight hotkey: bring Astrail up and open the launcher palette
            // from anywhere. The handler runs for our one registered shortcut.
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    
                    let settings = current_settings(app);
                    
                    let spot_s = parse_shortcut(&settings.shortcuts.spotlight);
                    let toggle_s = parse_shortcut(&settings.shortcuts.overlay_toggle);
                    let settings_s = parse_shortcut(&settings.shortcuts.overlay_settings);

                    if Some(*shortcut) == toggle_s {
                        toggle_overlay(app);
                    } else if Some(*shortcut) == settings_s {
                        // Create the overlay settings window on open / destroy it on
                        // close (it doesn't exist during gameplay, so we can't emit to it).
                        toggle_overlay_settings(app);
                    } else if Some(*shortcut) == spot_s {
                        show_main(app);
                        events::open_spotlight(app);
                    }
                })
                .build(),
        )
        .setup(|app| {

            
            let handle = app.handle().clone();
            let settings = storage::load_settings(&handle);
            // Apply the saved overlay config to the sampler before it starts.
            apply_overlay_settings(&handle, &settings);
            app.manage(std::sync::Mutex::new(settings.clone()));
            app.manage(PendingImport::default());

            // NOTE: the in-game overlay WebView window is intentionally **not** created
            // here. The HUD is drawn by the native layered window (no Chromium), so the
            // WebView is only needed for the settings screen — it's created on demand by
            // `ensure_overlay_window` and destroyed on close, so no WebView2 process sits
            // resident during gameplay. This is the key "lightweight overlay" change.

            // Migration: older builds autostarted the app elevated through a
            // `/RL HIGHEST` logon task. Replace it with the ordinary Run key. Only
            // an elevated process can delete that task, and the task itself only
            // launches the app elevated, so a normal launch spawns nothing here.
            #[cfg(windows)]
            if elevation::is_elevated() {
                std::thread::spawn(move || {
                    if elevation::remove_legacy_logon_task()
                        && !autostart::is_enabled().unwrap_or(false)
                    {
                        if let Err(e) = autostart::enable() {
                            log::warn!("could not move autostart to the Run key: {e}");
                        }
                    }
                });
            }

            // One-off cache maintenance, off the main thread: rename cover files
            // from the old unstable hash to FNV-1a, then keep `covers/` and
            // `app_icons/` under their size caps (they had none before, so they
            // grew forever); pull remote user covers onto disk so the CSP can
            // stop allowing images from any host; and move the autostart entry
            // from before the rename to Astrail's name, rewriting an unquoted
            // command line left by the old plugin.
            {
                let maintenance = handle.clone();
                std::thread::spawn(move || {
                    crate::art::migrate_filenames(&maintenance);
                    crate::art::prune_covers(&maintenance);
                    crate::appicons::maintain(&maintenance);
                    crate::art::migrate_remote_user_covers(&maintenance);
                    backup::auto_backup(&maintenance);
                    if let Err(e) = autostart::repair() {
                        log::warn!("could not repair the Run value: {e}");
                    }
                });
            }

            // Close any play sessions left dangling by a previous crash/force-quit,
            // then start the global watcher that times games however they launch.
            playtime::reconcile(&handle);
            // Load the saved Discord client id so the watcher can set Rich Presence,
            // and the opt-in flag that decides whether it may publish at all.
            discord::set_client_id(&storage::load_discord_client_id(&handle));
            discord::set_enabled(storage::load_settings(&handle).discord_enabled);
            // Start the metrics sampler (idle until the overlay is on and a game runs),
            // the PresentMon controller (idle until FPS is wanted + a game runs) and the
            // CPU-temp sidecar controller (idle until CPU temp is wanted + a game runs).
            metrics::start(handle.clone());
            presentmon::start(handle.clone());
            cputemp::start(handle.clone());
            playtime::start(handle.clone());
            // Foreground hook for games started outside Astrail. After the watcher:
            // its first notification (a game already in front) needs someone to wake.
            playtime::set_external_tracking(settings.track_external_games);
            // Register the global shortcuts
            register_shortcuts(&handle, &settings.shortcuts);

            // System tray: Astrail lives in the tray so the watchers keep running
            // after the window is closed (tray.rs).
            tray::build(&handle, &settings.language)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_library,
            resolve_covers,
            resolve_cover_hires,
            library_changed,
            set_cover,
            set_cover_image,
            clear_cover_cache,
            hide_game,
            unhide_game,
            get_hidden_library,
            hidden_count,
            restore_hidden,
            set_game_type,
            add_manual_app,
            remove_game,
            set_favorite,
            set_categories,
            list_categories,
            add_category,
            set_category_icon,
            remove_category,
            rename_category,
            set_category_order,
            get_playtime,
            all_playtime,
            cached_library,
            game_dir_size,
            steam_playtime,
            app_icons,
            get_discord_client_id,
            set_discord_client_id,
            get_autostart,
            set_autostart,
            get_app_settings,
            patch_app_settings,
            system_info,
            about_info,
            legal_document,
            overlay_mpo_diagnostics,
            username,
            metrics_access,
            restart_as_admin,
            prepare_for_update,
            abort_update,
            open_game_folder,
            open_external,
            user_screenshots,
            launch_game,
            set_overlay_interactive,
            show_main_window,
            main_window_visible,
            export_diagnostics,
            export_user_data,
            pick_user_data_backup,
            apply_user_data_backup,
            discard_user_data_backup,
            report_frontend_error,
            updates::check_update
        ])
        .build(context)
        .expect("failed to build the Tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                log::info!("Astrail exiting");
                shutdown_for_exit(app_handle);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AppSettings {
        serde_json::from_str("{}").expect("every settings field has a default")
    }

    #[test]
    fn a_poisoned_settings_lock_is_still_readable() {
        let state = std::sync::Arc::new(std::sync::Mutex::new(settings()));
        let poisoner = std::sync::Arc::clone(&state);
        let _ = std::thread::spawn(move || {
            let mut guard = poisoner.lock().expect("fresh mutex");
            guard.minimize_to_tray = false;
            panic!("poison the settings lock on purpose");
        })
        .join();
        assert!(state.is_poisoned());
        assert!(!lock_settings(&state).minimize_to_tray, "the last write survives the poison");
    }

    fn app_entry(id: &str, executable: Option<&str>) -> Game {
        Game {
            id: id.to_string(),
            name: id.to_string(),
            source: GameSource::App,
            app_id: None,
            executable: executable.map(str::to_string),
            install_dir: None,
            cover_url: None,
            launch_uri: None,
            favorite: false,
            categories: Vec::new(),
        }
    }

    #[test]
    fn icons_are_read_from_library_entries_never_from_a_path_the_webview_names() {
        // A23: `app_icon(path)` parsed any file the webview pointed at. The batch
        // takes ids; an id that is not in the library, or a path passed off as an
        // id, resolves to nothing.
        let manual = vec![app_entry("manual:notes", Some("C:\\Tools\\notes.exe"))];
        let cached = vec![
            app_entry("app:calc", Some("C:\\Apps\\calc.exe")),
            app_entry("manual:notes", Some("C:\\Stale\\notes.exe")),
            app_entry("app:blank", Some("  ")),
            app_entry("app:none", None),
        ];
        let ids: Vec<String> = [
            "app:calc",
            "C:\\Windows\\System32\\cmd.exe",
            "manual:notes",
            "app:blank",
            "app:none",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            icon_sources(&ids, &manual, &cached),
            vec![
                Some("C:\\Apps\\calc.exe".to_string()),
                None,
                Some("C:\\Tools\\notes.exe".to_string()),
                None,
                None,
            ],
            "same order and length as asked; the manual store wins over the cache"
        );
    }

    #[test]
    fn a_settings_patch_changes_only_the_fields_it_names() {
        // Regression (FE3): windows read-modify-wrote the whole struct and a
        // concurrent write from another window was lost. A patch leaves the rest.
        let mut current = settings();
        current.minimize_to_tray = false;
        current.overlay.show_ram = true;
        let next = apply_settings_patch(
            &current,
            serde_json::json!({ "overlay": { "show_fps": false }, "language": "en" }),
        )
        .unwrap();
        assert!(!next.overlay.show_fps);
        assert!(next.overlay.show_ram, "sibling overlay field kept");
        assert!(!next.minimize_to_tray, "top-level field kept");
        assert_eq!(next.language, "en");
        assert_eq!(next.shortcuts.spotlight, current.shortcuts.spotlight);
    }

    #[test]
    fn a_settings_patch_refuses_unknown_keys_and_wrong_types() {
        let current = settings();
        assert!(apply_settings_patch(&current, serde_json::json!({ "overlay": { "show_fsp": true } })).is_err());
        assert!(apply_settings_patch(&current, serde_json::json!({ "minimize_to_tray": "yes" })).is_err());
        assert!(apply_settings_patch(&current, serde_json::json!(true)).is_err());
    }

    #[test]
    fn only_a_logon_start_past_onboarding_with_close_to_tray_stays_hidden() {
        assert!(stays_in_tray(true, true, true));
        // A double click always opens the window.
        assert!(!stays_in_tray(false, true, true));
        // Onboarding must be seen, even at logon.
        assert!(!stays_in_tray(true, true, false));
        // Closing quits the app, so the tray is not its home: show it.
        assert!(!stays_in_tray(true, false, true));
    }

    #[test]
    fn the_update_channel_is_stable_unless_a_valid_patch_says_beta() {
        // Settings written before the channel existed must read as stable: beta is
        // opt-in, and an unknown channel name must not be stored.
        let current = settings();
        assert_eq!(current.update_channel, models::UpdateChannel::Stable);
        let beta = apply_settings_patch(&current, serde_json::json!({ "update_channel": "beta" })).unwrap();
        assert_eq!(beta.update_channel, models::UpdateChannel::Beta);
        assert!(apply_settings_patch(&current, serde_json::json!({ "update_channel": "nightly" })).is_err());
        assert!(apply_settings_patch(&current, serde_json::json!({ "update_channel": "Beta" })).is_err());
    }

    #[test]
    fn a_hud_option_the_backend_does_not_know_is_refused() {
        // Regression: position, font size and MPO mode were free strings, so a
        // patch could store `"mpo_mode": "banana"` and the HUD silently used the
        // default. The file loads such a value leniently; the patch must not.
        let current = settings();
        let moved = apply_settings_patch(
            &current,
            serde_json::json!({ "overlay": { "position": "bottom-right", "font_size": "base", "mpo_mode": "performance" } }),
        )
        .unwrap();
        assert_eq!(moved.overlay.position, models::OverlayPosition::BottomRight);
        assert_eq!(moved.overlay.font_size, models::HudFontSize::Base);
        assert_eq!(moved.overlay.mpo_mode, models::MpoMode::Performance);
        for bad in [
            serde_json::json!({ "overlay": { "mpo_mode": "banana" } }),
            serde_json::json!({ "overlay": { "position": "center" } }),
            serde_json::json!({ "overlay": { "font_size": "XS" } }),
            serde_json::json!({ "overlay": { "position": 3 } }),
        ] {
            let err = apply_settings_patch(&current, bad.clone()).unwrap_err();
            assert!(err.contains("overlay."), "{bad} -> {err}");
        }
    }

    #[test]
    fn games_started_outside_astrail_are_tracked_unless_a_patch_turns_it_off() {
        // A settings file written before the field existed must read as on: a
        // plain `#[serde(default)]` would silently disable it for every upgrade.
        let current = settings();
        assert!(current.track_external_games);
        let off = apply_settings_patch(&current, serde_json::json!({ "track_external_games": false })).unwrap();
        assert!(!off.track_external_games);
        assert!(apply_settings_patch(&current, serde_json::json!({ "track_external_games": "no" })).is_err());
    }

    #[test]
    fn the_escape_that_cancels_recording_is_never_a_global_hotkey() {
        // Regression (FE1): the recorder stored "ESCAPE" and it was registered as a
        // bare, machine-wide Escape hotkey.
        assert!(parse_shortcut("ESCAPE").is_none());
        assert!(parse_shortcut("Escape").is_none());
    }

    #[test]
    fn bare_keys_and_shift_letters_are_refused() {
        assert!(parse_shortcut("F9").is_none());
        assert!(parse_shortcut("A").is_none());
        assert!(parse_shortcut("Shift+A").is_none());
        assert!(parse_shortcut("Space").is_none());
    }

    #[test]
    fn modified_combinations_are_accepted() {
        assert!(parse_shortcut("Ctrl+Shift+F9").is_some());
        assert!(parse_shortcut("CommandOrControl+Shift+O").is_some());
        assert!(parse_shortcut("Alt+5").is_some());
        assert!(parse_shortcut("Super+Space").is_some());
        assert!(parse_shortcut("Shift+F5").is_some());
    }

    #[test]
    fn the_shipped_defaults_are_accepted() {
        let d = crate::models::ShortcutsSettings::default();
        for combo in [&d.spotlight, &d.overlay_toggle, &d.overlay_settings] {
            assert!(parse_shortcut(combo).is_some(), "{combo}");
        }
    }
}
