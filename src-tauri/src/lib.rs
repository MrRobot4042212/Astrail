#[cfg(windows)]
mod amd;
mod appicons;
mod apps_db;
mod art;
mod cputemp;
#[cfg(windows)]
mod elevation;
mod discord;
mod battlenet;
mod ea;
mod epic;
mod files;
mod fingerprint;
mod gog;
mod igdb;
mod jsonstore;
#[cfg(windows)]
mod jobobj;
mod launcher;
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
mod sidecar_integrity;
mod steam;
mod storage;
#[cfg(windows)]
mod sysstat;
mod system;
mod ubisoft;
mod windows_apps;
mod xbox;

use models::{Category, Game, GameSource, AppSettings};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

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
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Return the unified library across every supported source, sorted by name.
///
/// Each scanner runs independently: a failure in one source (store not
/// installed, corrupt manifest…) degrades to an empty list for that source
/// instead of failing the whole call. Sources are merged in priority order and
/// deduplicated by name, so a game owned on several stores shows up once with
/// the best available metadata (Steam first, since it ships CDN cover art).
#[tauri::command(async)]
async fn get_library(app: AppHandle) -> Result<Vec<Game>, String> {
    blocking(move || get_library_inner(app)).await
}

fn get_library_inner(app: AppHandle) -> Result<Vec<Game>, String> {
    let _span = perf::Span::new("get_library");
    // Computed *before* the scanners run, and stored afterwards. Taking it after
    // meant a game whose install finished mid-scan was absent from the results but
    // already reflected in the fingerprint, so `library_changed` answered "no" and
    // it stayed invisible until something else moved. It also keys the AppX cache.
    let fingerprint = fingerprint::compute();
    // Run all store scanners in parallel. Each is independent and failure-tolerant
    // (degrades to empty on missing store / corrupt data). Priority order is
    // preserved: store-specific scanners are extended first so they win dedup
    // over the generic windows_apps fallback. Manual apps are sequential after
    // because they need the AppHandle and are typically very few.
    let [steam, epic, gog, xbox, ea, ubisoft, bnet, win] =
        std::thread::scope(|s| {
            let t_steam   = s.spawn(|| steam::scan().unwrap_or_default());
            let t_epic    = s.spawn(|| epic::scan().unwrap_or_default());
            let t_gog     = s.spawn(|| gog::scan().unwrap_or_default());
            let t_xbox    = s.spawn(|| xbox::scan(fingerprint).unwrap_or_default());
            let t_ea      = s.spawn(|| ea::scan().unwrap_or_default());
            let t_ubisoft = s.spawn(|| ubisoft::scan().unwrap_or_default());
            // Battle.net before windows_apps so its richer per-flavor WoW entries
            // win the dedup over the single generic registry entry.
            let t_bnet    = s.spawn(|| battlenet::scan().unwrap_or_default());
            // Generic registry scan last so the dedup keeps the richer native entry.
            let t_win     = s.spawn(|| windows_apps::scan().unwrap_or_default());
            [
                t_steam.join().unwrap_or_default(),
                t_epic.join().unwrap_or_default(),
                t_gog.join().unwrap_or_default(),
                t_xbox.join().unwrap_or_default(),
                t_ea.join().unwrap_or_default(),
                t_ubisoft.join().unwrap_or_default(),
                t_bnet.join().unwrap_or_default(),
                t_win.join().unwrap_or_default(),
            ]
        });

    let mut games: Vec<Game> = Vec::new();
    games.extend(steam);
    games.extend(epic);
    games.extend(gog);
    games.extend(xbox);
    games.extend(ea);
    games.extend(ubisoft);
    games.extend(bnet);
    games.extend(win);
    games.extend(storage::load_manual(&app)?);

    // Deduplicate by name, keeping the first (highest-priority) occurrence.
    let mut seen = HashSet::new();
    games.retain(|g| seen.insert(g.name.to_lowercase()));

    // Drop entries the user has hidden (false positives from the generic scan).
    let hidden = storage::load_hidden(&app);
    if !hidden.is_empty() {
        let (visible, hidden_games): (Vec<Game>, Vec<Game>) = games.into_iter().partition(|g| !hidden.iter().any(|h| h == &g.id));
        games = visible;
        let _ = storage::save_hidden_cache(&app, &hidden_games);
    } else {
        let _ = storage::save_hidden_cache(&app, &[]);
    }

    // User-set cover overrides win over whatever each source provided.
    let overrides = storage::load_cover_overrides(&app);
    if !overrides.is_empty() {
        for game in &mut games {
            if let Some(url) = overrides.get(&game.id) {
                game.cover_url = Some(url.clone());
            }
        }
    }

    // User overlays: favorites and manual categories, keyed by game id.
    let favorites = storage::load_favorites(&app);
    let categories = storage::load_categories(&app);
    if !favorites.is_empty() || !categories.is_empty() {
        for game in &mut games {
            if favorites.iter().any(|f| f == &game.id) {
                game.favorite = true;
            }
            if let Some(cats) = categories.get(&game.id) {
                game.categories = cats.clone();
            }
        }
    }

    // User override: reclassify an entry as app/game (fixes mis-detection). Since
    // the library is re-scanned each call, "game" only needs to undo an App
    // detection (store sources are already games), so the real source is kept
    // whenever possible and removing the override self-heals on the next scan.
    let type_overrides = storage::load_type_overrides(&app);
    if !type_overrides.is_empty() {
        for game in &mut games {
            match type_overrides.get(&game.id).map(String::as_str) {
                Some("app") => game.source = GameSource::App,
                Some("game") if game.source == GameSource::App => {
                    game.source = GameSource::Windows;
                }
                _ => {}
            }
        }
    }

    // Fill in covers we already downloaded. Without this the frontend re-queued
    // every non-Steam entry through `resolve_cover` on *every* refresh, even
    // though the image was sitting on disk: N IPC round-trips and N re-renders
    // for nothing.
    for game in &mut games {
        if game.cover_url.is_none() && game.source != GameSource::App {
            game.cover_url = art::cached_path(&app, &game.name);
        }
    }

    // Sort by a precomputed lowercase key: `sort_by` with `to_lowercase()`
    // inside allocated two Strings per comparison (O(n log n) allocations).
    let mut keyed: Vec<(String, Game)> = games
        .into_iter()
        .map(|g| (g.name.to_lowercase(), g))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let games: Vec<Game> = keyed.into_iter().map(|(_, g)| g).collect();

    // Persist for instant startup next time and for the playtime watcher's index.
    write_library_cache(&app, &games);
    // Remember what the stores looked like, so `library_changed` can answer
    // without redoing any of this.
    let _ = jsonstore::save(&app, FINGERPRINT_FILE, &fingerprint);
    // The watcher re-reads the index when this file changes; nudge it so a game
    // launched right after a scan is picked up immediately.
    playtime::wake();
    Ok(games)
}

/// Name of the on-disk snapshot of the last computed library. It is a contract,
/// not a cache: the playtime watcher reads it to know what to match processes
/// against, and `resolve_game` resolves command ids through it.
const LIBRARY_CACHE_FILE: &str = "library_cache.json";
/// Fingerprint of the installed-games sources at the last successful scan.
const FINGERPRINT_FILE: &str = "library_fingerprint.json";

fn write_library_cache(app: &AppHandle, games: &[Game]) {
    if let Err(e) = jsonstore::save(app, LIBRARY_CACHE_FILE, &games) {
        eprintln!("[library] could not write {LIBRARY_CACHE_FILE}: {e}");
    }
}

/// The last computed library from disk (empty if never scanned or unreadable).
fn read_library_cache(app: &AppHandle) -> Vec<Game> {
    jsonstore::load_or_default(app, LIBRARY_CACHE_FILE)
}

/// Resolve a library entry **in Rust** from an id the frontend sent.
///
/// Commands take ids, never whole `Game` structs: a struct coming from the
/// webview is attacker-controlled input that would otherwise flow straight into
/// `ShellExecuteW`/`CreateProcess` (see `launcher.rs`). The manual store wins
/// over the cache because it is the authoritative record for `manual:` entries.
fn resolve_game(app: &AppHandle, id: &str) -> Option<Game> {
    if let Ok(manual) = storage::load_manual(app) {
        if let Some(game) = manual.into_iter().find(|g| g.id == id) {
            return Some(game);
        }
    }
    read_library_cache(app).into_iter().find(|g| g.id == id)
}

/// Folder to reveal for an entry: its install dir, or the executable parent.
fn game_folder(game: &Game) -> Option<String> {
    if let Some(dir) = game.install_dir.as_deref().filter(|d| !d.trim().is_empty()) {
        return Some(dir.to_string());
    }
    let exe = game.executable.as_deref().filter(|e| !e.trim().is_empty())?;
    std::path::Path::new(exe)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
}

/// The last computed library from disk (empty if never scanned). The frontend
/// paints this instantly, then calls `get_library` to refresh in the background.
#[tauri::command(async)]
fn cached_library(app: AppHandle) -> Result<Vec<Game>, String> {
    Ok(read_library_cache(&app))
}

/// Set a manual cover URL for a game id (empty/None clears it). Overrides always
/// take precedence over auto-resolved artwork.
#[tauri::command(async)]
fn set_cover(app: AppHandle, id: String, url: Option<String>) -> Result<(), String> {
    storage::set_cover_override(&app, &id, url.as_deref())
}

/// Save a dropped/picked local image as a game's cover and set it as the override.
/// Returns the saved local path (rendered via the asset protocol).
#[tauri::command(async)]
fn set_cover_image(
    app: AppHandle,
    id: String,
    data: Vec<u8>,
    ext: String,
) -> Result<String, String> {
    let path = storage::save_cover_image(&app, &id, &data, &ext)?;
    storage::set_cover_override(&app, &id, Some(&path))?;
    Ok(path)
}

/// Resolve a cover image for a game name via IGDB, cached on disk. The frontend
/// calls this lazily for entries without artwork.
#[tauri::command(async)]
async fn resolve_cover(app: AppHandle, name: String) -> Result<Option<String>, String> {
    blocking(move || Ok(art::resolve(&app, &name))).await
}

/// Whether anything that feeds the library has changed since the last scan.
///
/// The frontend calls this before its periodic refresh: when nothing moved, the
/// whole 8-scanner + PowerShell pass is skipped. Conservative — any source it
/// cannot read counts as changed.
#[tauri::command(async)]
async fn library_changed(app: AppHandle) -> Result<bool, String> {
    blocking(move || {
        let current = fingerprint::compute();
        let stored: u64 = jsonstore::load_or_default(&app, FINGERPRINT_FILE);
        Ok(stored == 0 || stored != current)
    })
    .await
}

/// Resolve the high-resolution cover for the detail page hero. Reuses the cached
/// IGDB image id, so at most it downloads one image — never a new search.
#[tauri::command(async)]
async fn resolve_cover_hires(app: AppHandle, name: String) -> Result<Option<String>, String> {
    blocking(move || Ok(art::resolve_hires(&app, &name))).await
}

/// Wipe the cover cache (URLs + downloaded images) so everything re-resolves.
#[tauri::command(async)]
async fn clear_cover_cache(app: AppHandle) -> Result<(), String> {
    blocking(move || art::clear_cache(&app)).await
}

/// Reclassify an entry as an application or a game (`"app"` / `"game"`), or clear
/// the override (any other value) to fall back to auto-detection.
#[tauri::command(async)]
fn set_game_type(app: AppHandle, id: String, kind: String) -> Result<(), String> {
    let k = kind.as_str();
    storage::set_type_override(&app, &id, if k == "app" || k == "game" { Some(k) } else { None })
}

/// Hide a game from the library (e.g. a non-game picked up by the registry scan).
#[tauri::command(async)]
fn hide_game(app: AppHandle, id: String) -> Result<(), String> {
    storage::set_hidden(&app, &id, true)
}

/// Unhide a game from the library.
#[tauri::command(async)]
fn unhide_game(app: AppHandle, id: String) -> Result<(), String> {
    storage::set_hidden(&app, &id, false)
}

/// Get the cached metadata of hidden games.
#[tauri::command(async)]
fn get_hidden_library(app: AppHandle) -> Result<Vec<Game>, String> {
    storage::load_hidden_cache(&app)
}

/// Number of currently-hidden games (shown in settings so they can be restored).
#[tauri::command(async)]
fn hidden_count(app: AppHandle) -> Result<usize, String> {
    Ok(storage::load_hidden(&app).len())
}

/// Restore every hidden game.
#[tauri::command(async)]
fn restore_hidden(app: AppHandle) -> Result<(), String> {
    storage::clear_hidden(&app)
}

#[tauri::command(async)]
fn add_manual_app(
    app: AppHandle,
    name: String,
    executable: String,
    cover_url: Option<String>,
) -> Result<Game, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }

    let mut manual = storage::load_manual(&app)?;
    let id = format!(
        "manual:{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis()
    );

    let game = Game {
        id,
        name,
        source: GameSource::Manual,
        app_id: None,
        executable: Some(executable),
        install_dir: None,
        cover_url: cover_url.filter(|s| !s.trim().is_empty()),
        launch_uri: None,
        favorite: false,
        categories: Vec::new(),

    };

    manual.push(game.clone());
    storage::save_manual(&app, &manual)?;
    Ok(game)
}

/// Remove a manually-added app. Store-managed entries are ignored.
#[tauri::command(async)]
fn remove_game(app: AppHandle, id: String) -> Result<(), String> {
    let mut manual = storage::load_manual(&app)?;
    let before = manual.len();
    manual.retain(|g| g.id != id);
    if manual.len() != before {
        storage::save_manual(&app, &manual)?;
    }
    Ok(())
}

/// Mark or unmark a game as favorite (applies to any source, not just manual).
#[tauri::command(async)]
fn set_favorite(app: AppHandle, id: String, favorite: bool) -> Result<(), String> {
    storage::set_favorite(&app, &id, favorite)
}

/// Replace the manual category list for a game id (empty list clears it).
#[tauri::command(async)]
fn set_categories(app: AppHandle, id: String, categories: Vec<String>) -> Result<(), String> {
    storage::set_categories(&app, &id, &categories)
}

/// Every explicitly-created category with its icon (persist even with zero games).
#[tauri::command(async)]
fn list_categories(app: AppHandle) -> Result<Vec<Category>, String> {
    Ok(storage::load_categories_meta(&app))
}

/// Create a category by name, optionally with an icon key from the bundled set.
#[tauri::command(async)]
fn add_category(app: AppHandle, name: String, icon: Option<String>) -> Result<(), String> {
    storage::add_category_name(&app, &name, icon.as_deref())
}

/// Set (or clear, with None) the icon key for an existing category.
#[tauri::command(async)]
fn set_category_icon(app: AppHandle, name: String, icon: Option<String>) -> Result<(), String> {
    storage::set_category_icon(&app, &name, icon.as_deref())
}

/// Delete a category and strip it from every game.
#[tauri::command(async)]
fn remove_category(app: AppHandle, name: String) -> Result<(), String> {
    storage::remove_category_name(&app, &name)
}

/// Rename a category everywhere (merges if the new name already exists).
#[tauri::command(async)]
fn rename_category(app: AppHandle, old: String, new: String) -> Result<(), String> {
    storage::rename_category_name(&app, &old, &new)
}

/// Persist the explicit category order (as shown in the sidebar).
#[tauri::command(async)]
fn set_category_order(app: AppHandle, names: Vec<String>) -> Result<(), String> {
    storage::set_category_order(&app, &names)
}

/// Accumulated play stats (seconds + last played) for a game id.
#[tauri::command(async)]
fn get_playtime(app: AppHandle, id: String) -> Result<playtime::PlayStat, String> {
    Ok(playtime::get(&app, &id))
}

/// Play stats for every tracked game id (for sorting the library).
#[tauri::command(async)]
fn all_playtime(
    app: AppHandle,
) -> Result<std::collections::HashMap<String, playtime::PlayStat>, String> {
    Ok(playtime::all(&app))
}

/// Size on disk of a library entry install folder, for the detail page.
///
/// Takes an id, not a path: `install_dir` is read from our own library cache and
/// validated, so the webview cannot ask for the size of an arbitrary directory.
/// `None` = the entry has no known folder.
#[tauri::command(async)]
async fn game_dir_size(app: AppHandle, id: String) -> Result<Option<u64>, String> {
    blocking(move || {
        let Some(game) = resolve_game(&app, &id) else {
            return Err(format!("Entrada desconocida: {id}"));
        };
        let Some(folder) = game_folder(&game) else {
            return Ok(None);
        };
        let dir = files::validate_dir(&folder)?;
        Ok(Some(files::dir_size(&dir)))
    })
    .await
}

/// Extract the real icon embedded in an app's executable (cached PNG path), used
/// as the icon for apps without a cover or known brand logo.
#[tauri::command(async)]
async fn app_icon(app: AppHandle, path: String) -> Result<Option<String>, String> {
    blocking(move || Ok(appicons::extract(&app, &path))).await
}

/// Seconds the main window must stay hidden before we ask WebView2 to trim its
/// memory. Short enough to matter when Meteor lives in the tray, long enough not
/// to fire on a hide/show bounce.
#[cfg(windows)]
const WEBVIEW_TRIM_DELAY_SECS: u64 = 10;

/// Ask WebView2 to drop what it can (`LOW`) or go back to normal.
///
/// Closing the window only hides it — the whole Chromium process tree stays
/// resident so the playtime/Discord watchers keep running. `LOW` lets the engine
/// release caches and decoded images while nobody is looking, and unlike
/// `TrySuspend` it has no lifecycle semantics that could break the page state.
#[cfg(windows)]
fn set_webview_memory_low(app: &AppHandle, low: bool) {
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
fn emit_visibility(app: &AppHandle, visible: bool) {
    let _ = app.emit("window-visibility", visible);
}

/// Reveal the main window once the frontend has painted.
///
/// The window is created with `"visible": false` so the user never sees an empty
/// white rectangle while the webview boots (it is also `maximized` + `center`,
/// which made that flash very visible).
// NOT `async`: shows a window, which belongs on the main thread.
#[tauri::command]
fn show_main_window(app: AppHandle) {
    show_main(&app);
}

/// Bring the main window to the front (used by the tray and Spotlight).
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        #[cfg(windows)]
        set_webview_memory_low(app, false);
        emit_visibility(app, true);
    }
}

/// Whether Meteor is set to launch on Windows login (the autostart `Run` key).
#[tauri::command(async)]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("Failed to read autostart: {e}"))
}

/// Autostart is the `Run` key only, elevated or not: Meteor never starts itself
/// elevated at logon (see `elevation::remove_legacy_logon_task`).
#[tauri::command(async)]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let auto = app.autolaunch();

    // Idempotente: si ya está en el estado pedido no hacemos nada. Evita que
    // `disable()` falle con "el sistema no puede encontrar el archivo
    // especificado (os error 2)" al borrar la clave Run del registro cuando
    // nunca estuvo activado (y lo simétrico al activar uno ya activo).
    let already = auto.is_enabled().unwrap_or(false);
    if enabled == already {
        return Ok(());
    }
    if enabled {
        auto.enable().map_err(|e| e.to_string())
    } else {
        auto.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn get_app_settings(state: tauri::State<'_, std::sync::Mutex<AppSettings>>) -> Result<AppSettings, String> {
    Ok(state.lock().unwrap().clone())
}

#[tauri::command(async)]
fn system_info() -> Result<system::SystemInfo, String> {
    Ok(system::collect())
}

/// Overlay MPO diagnostics: live composition health + the system-config levers
/// (monitor count, mixed refresh, HAGS) that decide whether the HUD can run on a
/// hardware overlay plane (free) or gets composited by DWM (costing the game's FPS).
#[tauri::command(async)]
fn overlay_mpo_diagnostics() -> system::MpoDiagnostics {
    system::mpo_diagnostics()
}

/// The current OS user's name, for greetings. Prefers the Windows display/full
/// name (e.g. "Diego Chicoma"); falls back to the login name (USERNAME). Empty
/// string if nothing is available.
// Async: `GetUserNameExW(NameDisplay)` can round-trip to a domain controller.
#[tauri::command(async)]
fn username() -> String {
    #[cfg(windows)]
    {
        if let Some(name) = system::display_name() {
            return name;
        }
    }
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// Whether Meteor is running elevated (admin). Admin is required for CPU temp and
/// for FPS on NVIDIA (PresentMon). False on non-Windows.
#[tauri::command]
fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        elevation::is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Flush pending state and stop both sidecars properly. Termination is not a
/// shutdown for either of them: cputemp would leave the LibreHardwareMonitor kernel
/// driver loaded and registered for the rest of the boot, and PresentMon would leave
/// its ETW realtime session live with its buffers pinned until reboot. The
/// kill-on-close job (`jobobj.rs`) stays as the crash backstop, which is all it can
/// be — TerminateProcess cannot be intercepted.
fn shutdown_for_exit(app: &AppHandle) {
    // The URL cache is written at most every 2 s during a cover pass; make sure the
    // last entries are not lost.
    crate::art::flush(app);
    #[cfg(windows)]
    {
        crate::presentmon::shutdown();
        crate::cputemp::shutdown();
    }
}

/// Called right before the updater installs. The plugin's `install` ends in
/// `std::process::exit`, so `RunEvent::Exit` never fires on that path; without this
/// an update taken with admin metrics on left the ETW session and the kernel driver
/// behind. The sidecars stay suspended until `abort_update` (the install failed) or
/// the process exits.
#[tauri::command]
async fn prepare_for_update(app: AppHandle) -> Result<(), String> {
    crate::metrics::set_sidecars_suspended(true);
    blocking(move || {
        shutdown_for_exit(&app);
        Ok(())
    })
    .await
}

/// The install did not happen after `prepare_for_update`: let the sidecars run again.
#[tauri::command]
fn abort_update() {
    crate::metrics::set_sidecars_suspended(false);
}

/// Relaunch Meteor as administrator (UAC prompt), then exit this instance.
#[tauri::command]
fn restart_as_admin(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        elevation::relaunch_elevated()?;
        // Let the command response flush, then quit so only the elevated copy runs.
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            handle.exit(0);
        });
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Solo disponible en Windows.".into())
    }
}

/// Change only the settings named in `patch`, atomically.
///
/// There is deliberately no whole-struct setter. A window that reads the settings,
/// spreads its change over them and writes everything back loses a write made by
/// another window in between (the launcher and the in-game screen can both be
/// open). The merge happens here, under the settings lock, and only the parts
/// that changed are re-applied: a HUD color does not re-register the hotkeys.
#[tauri::command(async)]
fn patch_app_settings(
    app: AppHandle,
    state: tauri::State<'_, std::sync::Mutex<AppSettings>>,
    patch: serde_json::Value,
) -> Result<(), String> {
    let (previous, next) = {
        let mut current = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = apply_settings_patch(&current, patch)?;
        storage::save_settings(&app, &next);
        (std::mem::replace(&mut *current, next.clone()), next)
    };
    settings_changed(&app, &previous, &next);
    Ok(())
}

/// Merge a partial settings object into `current`. Nested objects merge key by
/// key; any key the settings do not have is an error rather than silently ignored.
fn apply_settings_patch(current: &AppSettings, patch: serde_json::Value) -> Result<AppSettings, String> {
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
    let mut merged = serde_json::to_value(current).map_err(|e| e.to_string())?;
    merge(&mut merged, patch, "")?;
    serde_json::from_value(merged).map_err(|e| format!("Invalid settings patch: {e}"))
}

/// Re-apply what changed between two settings snapshots and notify every window.
fn settings_changed(app: &AppHandle, previous: &AppSettings, next: &AppSettings) {
    fn same<T: serde::Serialize>(a: &T, b: &T) -> bool {
        matches!((serde_json::to_value(a), serde_json::to_value(b)), (Ok(a), Ok(b)) if a == b)
    }
    if !same(&previous.overlay, &next.overlay) {
        apply_overlay_settings(app, next);
    }
    discord::set_enabled(next.discord_enabled);
    if !same(&previous.shortcuts, &next.shortcuts) {
        // A window-manager operation: keep it on the main thread.
        let handle = app.clone();
        let shortcuts = next.shortcuts.clone();
        let _ = app.run_on_main_thread(move || register_shortcuts(&handle, &shortcuts));
    }
    let _ = app.emit("settings-updated", ());
}

/// Parse a stored combination into a global shortcut, refusing unsafe ones.
///
/// `RegisterHotKey` takes the key away from every application on the machine, games
/// included. The settings recorder used to store whatever key it saw, including the
/// Escape pressed to *cancel* recording, and this parser accepted it, so a bare
/// `Escape` became a system-wide hotkey. See `is_safe_global_shortcut`.
fn parse_shortcut(s: &str) -> Option<tauri_plugin_global_shortcut::Shortcut> {
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
fn is_safe_global_shortcut(sc: &tauri_plugin_global_shortcut::Shortcut) -> bool {
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

fn register_shortcuts(app: &AppHandle, shortcuts: &crate::models::ShortcutsSettings) {
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
            eprintln!(
                "[shortcuts] {what}: '{combo}' is not a valid combination or lacks a Ctrl/Alt/Win modifier; not registered"
            );
            continue;
        };
        if let Err(e) = app.global_shortcut().register(parsed) {
            eprintln!(
                "[shortcuts] {what}: could not register '{combo}' (another application may own it): {e}"
            );
        }
    }
}

/// Cover the monitor the game is on with the in-game settings window.
///
/// The foreground window is still the game when the hotkey fires (the settings
/// window is created unfocused). It used to take `primary_monitor()`, so on a
/// multi-monitor setup the settings screen opened on another display than the game.
fn cover_game_monitor(w: &tauri::WebviewWindow) {
    use tauri::{PhysicalPosition, PhysicalSize};
    let mon = metrics::monitor_geometry(overlay::foreground());
    let _ = w.set_size(PhysicalSize::new(mon.width.max(1) as u32, mon.height.max(1) as u32));
    let _ = w.set_position(PhysicalPosition::new(mon.left, mon.top));
}

/// Apply the overlay config live: update the sampler and snapshot the full config
/// for the native HUD renderer. The native HUD (drawn by the sampler) reflects this
/// on its next tick; when `enabled` is false the sampler hides it.
fn apply_overlay_settings(_app: &AppHandle, settings: &AppSettings) {
    let fps_wanted = settings.overlay.show_fps || settings.overlay.show_frametime;
    metrics::configure(
        settings.overlay.enabled,
        settings.overlay.interval_ms,
        fps_wanted,
        settings.overlay.show_cpu_temp,
    );
    metrics::set_gpu(settings.overlay.gpu.clone());
    // Snapshot the full overlay config (colors, position, font size, opacity, which
    // metrics) so the native HUD renderer reads it each tick.
    metrics::set_render_cfg(settings.overlay.clone());
}

/// Toggle the overlay on/off (the global hotkey). Persists and applies live.
fn toggle_overlay(app: &AppHandle) {
    if let Some(state) = app.try_state::<std::sync::Mutex<AppSettings>>() {
        // One lock for the whole read-modify-write, so a settings save landing
        // between the read and the write is not overwritten.
        let s = {
            let mut current = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            current.overlay.enabled = !current.overlay.enabled;
            storage::save_settings(app, &current);
            current.clone()
        };
        apply_overlay_settings(app, &s);
        // An open settings screen shows the toggle too.
        let _ = app.emit("settings-updated", ());
    }
}

/// Create the in-game overlay *settings* WebView window on demand. It is **not**
/// created at startup and is destroyed when the settings screen closes, so a full
/// WebView2/Chromium stack (browser + renderer + GPU process) never sits resident
/// during gameplay — the HUD itself is drawn by the native window, so this window's
/// only purpose is the settings screen. Returns the existing window if already open.
fn ensure_overlay_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window("overlay") {
        return Some(w);
    }
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    // Its own document, not `index.html`: pointing both windows at the launcher's
    // entry made the overlay download and evaluate the entire launcher bundle
    // (~854 KB of eager JS, the grid and both catalogs included) to draw one
    // settings panel while a game is running. Next code-splits per route, so this
    // loads only the overlay tree.
    WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
        .title("Meteor Overlay")
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
fn toggle_overlay_settings(app: &AppHandle) {
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
fn set_overlay_interactive(app: AppHandle, interactive: bool) -> Result<(), String> {
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
/// The saved Discord Rich Presence client id (empty = disabled).
#[tauri::command(async)]
fn get_discord_client_id(app: AppHandle) -> Result<String, String> {
    Ok(storage::load_discord_client_id(&app))
}

/// Save the Discord client id and apply it live to the presence watcher.
#[tauri::command(async)]
fn set_discord_client_id(app: AppHandle, id: String) -> Result<(), String> {
    storage::save_discord_client_id(&app, &id)?;
    discord::set_client_id(&id);
    Ok(())
}

/// Reveal a library entry folder in the OS file manager.
///
/// Id-based for the same reason as `game_dir_size`: the old `open_path(path)`
/// handed an arbitrary webview-supplied string to Explorer.
#[tauri::command(async)]
fn open_game_folder(app: AppHandle, id: String) -> Result<(), String> {
    let Some(game) = resolve_game(&app, &id) else {
        return Err(format!("Entrada desconocida: {id}"));
    };
    let folder = game_folder(&game).ok_or_else(|| "La entrada no tiene carpeta".to_string())?;
    let dir = files::validate_dir(&folder)?;
    files::open_folder(&dir)
}

/// Open one of the detail page's community links in the user's browser.
///
/// Validated against a host allowlist in Rust: a bare `<a href>` in the webview
/// navigates the app window itself, and the target host must not be whatever a
/// library entry happens to contain.
#[tauri::command(async)]
fn open_external(url: String) -> Result<(), String> {
    files::open_external(&url)
}

/// The user's own screenshots for a game (Steam + Windows Game Bar).
#[tauri::command(async)]
fn user_screenshots(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    let Some(game) = resolve_game(&app, &id) else {
        return Err(format!("Entrada desconocida: {id}"));
    };
    Ok(screenshots::user_screenshots(&app, &game))
}

/// Launch a library entry by id.
///
/// The entry is re-resolved from the manual store / library cache here, so the
/// executable and protocol URI that reach `launcher.rs` are always ones a
/// scanner produced -- never a struct the webview built.
///
/// Two steps on two threads. Resolving parses the whole `library_cache.json`,
/// which grows with the library, so it runs on the blocking pool instead of
/// freezing IPC, the tray and the shortcuts. The launch itself goes back to the
/// main thread: `ShellExecuteW` may hand it to a Shell extension that needs a
/// COM single-threaded apartment, which the main thread has (the webview runtime
/// initializes it) and the blocking pool does not.
#[tauri::command]
async fn launch_game(app: AppHandle, id: String) -> Result<(), String> {
    let resolver = app.clone();
    let game = blocking(move || {
        resolve_game(&resolver, &id).ok_or_else(|| format!("Unknown library entry: {id}"))
    })
    .await?;
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(launcher::launch(&game));
    })
    .map_err(|e| format!("Could not reach the main thread: {e}"))?;
    // Playtime is accumulated by the watcher (see `playtime::start`), which
    // `launcher::launch` notifies before it starts the process.
    blocking(move || {
        rx.recv()
            .unwrap_or_else(|_| Err("The launch was dropped before it ran".into()))
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    elevation::await_previous_instance();

    tauri::Builder::default()
        // First, so a second launch exits before it registers a tray icon or global
        // hotkeys, or starts a PresentMon whose `--stop_existing_session` would end
        // this instance's ETW session. The second launch just surfaces this window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // Launch on login (Windows registry Run key). The MacosLauncher arg is
        // ignored on Windows; no launch args needed.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // In-app auto-update (checks GitHub Releases) + relaunch after install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Closing the main window hides Meteor to the tray instead of quitting,
        // so the playtime/Discord/Spotlight watchers keep running. Real quit is
        // the tray's "Salir" item.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let minimize = window
                        .app_handle()
                        .try_state::<std::sync::Mutex<AppSettings>>()
                        .map(|s| s.lock().unwrap().minimize_to_tray)
                        .unwrap_or(true);
                    
                    if minimize {
                        api.prevent_close();
                        let _ = window.hide();
                        let app = window.app_handle().clone();
                        emit_visibility(&app, false);
                        // Trim WebView2's memory once it has been hidden for a
                        // while, and only if it is still hidden by then.
                        #[cfg(windows)]
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(
                                WEBVIEW_TRIM_DELAY_SECS,
                            ));
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
                    } else {
                        // Let it close, which exits the app.
                    }
                }
            }
        })
        .plugin(
            // Global Spotlight hotkey: bring Meteor up and open the launcher palette
            // from anywhere. The handler runs for our one registered shortcut.
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    
                    let settings = app.try_state::<std::sync::Mutex<crate::models::AppSettings>>()
                        .map(|s| s.lock().unwrap().clone())
                        .unwrap_or_else(|| crate::storage::load_settings(app));
                    
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
                        let _ = app.emit("open-spotlight", ());
                    }
                })
                .build(),
        )
        .setup(|app| {

            
            let handle = app.handle().clone();
            let settings = storage::load_settings(&handle);
            // Apply the saved overlay config to the sampler before it starts.
            metrics::configure(
                settings.overlay.enabled,
                settings.overlay.interval_ms,
                settings.overlay.show_fps || settings.overlay.show_frametime,
                settings.overlay.show_cpu_temp,
            );
            metrics::set_gpu(settings.overlay.gpu.clone());
            // Snapshot the full overlay config for the native HUD renderer.
            metrics::set_render_cfg(settings.overlay.clone());
            app.manage(std::sync::Mutex::new(settings.clone()));

            // NOTE: the in-game overlay WebView window is intentionally **not** created
            // here. The HUD is drawn by the native layered window (no Chromium), so the
            // WebView is only needed for the settings screen — it's created on demand by
            // `ensure_overlay_window` and destroyed on close, so no WebView2 process sits
            // resident during gameplay. This is the key "lightweight overlay" change.

            // Migration: older builds autostarted an elevated Meteor through a
            // `/RL HIGHEST` logon task. Replace it with the ordinary Run key. Only
            // an elevated process can delete that task, and the task itself only
            // launches Meteor elevated, so a normal launch spawns nothing here.
            #[cfg(windows)]
            if elevation::is_elevated() {
                let migrate = handle.clone();
                std::thread::spawn(move || {
                    use tauri_plugin_autostart::ManagerExt;
                    if elevation::remove_legacy_logon_task()
                        && !migrate.autolaunch().is_enabled().unwrap_or(false)
                    {
                        if let Err(e) = migrate.autolaunch().enable() {
                            eprintln!("[autostart] could not move autostart to the Run key: {e}");
                        }
                    }
                });
            }

            // One-off cache maintenance, off the main thread: rename cover files
            // from the old unstable hash to FNV-1a, then keep `covers/` under its
            // size cap (it had none before, so it grew forever).
            {
                let maintenance = handle.clone();
                std::thread::spawn(move || {
                    crate::art::migrate_filenames(&maintenance);
                    crate::art::prune_covers(&maintenance);
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
            // Register the global shortcuts
            register_shortcuts(&handle, &settings.shortcuts);

            // System tray: Meteor lives in the tray so the watchers keep running
            // after the window is closed. Left-click or "Mostrar Meteor" reopens
            // the window; "Salir" really quits.
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show = MenuItem::with_id(app, "show", "Mostrar Meteor", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show, &quit])?;
                // No `unwrap()`: a missing icon must not take the whole app down
                // at startup — the tray just shows the default one.
                let mut tray = TrayIconBuilder::with_id("main");
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                let _tray = tray
                    .tooltip("Meteor")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_main(app),
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main(tray.app_handle());
                        }
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_library,
            resolve_cover,
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
            app_icon,
            get_discord_client_id,
            set_discord_client_id,
            get_autostart,
            set_autostart,
            get_app_settings,
            patch_app_settings,
            system_info,
            overlay_mpo_diagnostics,
            username,
            is_elevated,
            restart_as_admin,
            prepare_for_update,
            abort_update,
            open_game_folder,
            open_external,
            user_screenshots,
            launch_game,
            set_overlay_interactive,
            show_main_window
        ])
        .build(tauri::generate_context!())
        .expect("error al iniciar la aplicación Tauri")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
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
