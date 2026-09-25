// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

mod about;
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
use tauri::{AppHandle, Manager};

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

/// A store that fails degrades to an empty list (scanners are best-effort), but
/// the reason is kept: "Steam is not installed" and "the library folders could not
/// be read" look the same from the grid.
fn scanned(store: &str, result: Result<Vec<Game>, String>) -> Vec<Game> {
    result.unwrap_or_else(|e| {
        log::info!("{store} scan returned nothing: {e}");
        Vec::new()
    })
}

fn get_library_inner(app: AppHandle) -> Result<Vec<Game>, String> {
    let _span = perf::Span::new("get_library");
    let started = std::time::Instant::now();
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
            let t_steam   = s.spawn(|| scanned("steam", steam::scan()));
            let t_epic    = s.spawn(|| scanned("epic", epic::scan()));
            let t_gog     = s.spawn(|| scanned("gog", gog::scan()));
            let t_xbox    = s.spawn(|| scanned("xbox", xbox::scan(fingerprint)));
            let t_ea      = s.spawn(|| scanned("ea", ea::scan()));
            let t_ubisoft = s.spawn(|| scanned("ubisoft", ubisoft::scan()));
            // Battle.net before windows_apps so its richer per-flavor WoW entries
            // win the dedup over the single generic registry entry.
            let t_bnet    = s.spawn(|| scanned("battlenet", battlenet::scan()));
            // Generic registry scan last so the dedup keeps the richer native entry.
            let t_win     = s.spawn(|| scanned("windows_apps", windows_apps::scan()));
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

    log::info!(
        "library scan: steam={} epic={} gog={} xbox={} ea={} ubisoft={} battlenet={} windows_apps={} in {:?}",
        steam.len(),
        epic.len(),
        gog.len(),
        xbox.len(),
        ea.len(),
        ubisoft.len(),
        bnet.len(),
        win.len(),
        started.elapsed()
    );

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

    // Collapse same-name entries (the store copy beats the generic registry
    // copy) and remember which ids were folded, so overlays the user stored
    // under a dropped id still apply. Manual entries are never dropped.
    let library::Merged { mut games, aliases } = library::merge_duplicates(games);

    // Drop entries the user has hidden (false positives from the generic scan).
    // Strictly by id: see `library.rs` for why hidden ids are not aliased.
    let hidden = storage::load_hidden(&app);
    if !hidden.is_empty() {
        let (visible, hidden_games): (Vec<Game>, Vec<Game>) = games.into_iter().partition(|g| !hidden.iter().any(|h| h == &g.id));
        games = visible;
        let _ = storage::save_hidden_cache(&app, &hidden_games);
    } else {
        let _ = storage::save_hidden_cache(&app, &[]);
    }

    // User overlays (cover overrides, favorites, categories, app/game
    // reclassification) win over whatever each source provided.
    let overlays = library::Overlays {
        favorites: storage::load_favorites(&app),
        categories: storage::load_categories(&app),
        covers: storage::load_cover_overrides(&app),
        types: storage::load_type_overrides(&app),
    };
    library::apply_overlays(&mut games, &aliases, &overlays);
    // Play stats are read through the same aliases; a changed map changes what
    // the library shows for a game, so the webview re-reads them.
    if playtime::set_aliases(&aliases) {
        events::playtime_updated(&app, None);
    }

    // Fill in covers we already downloaded. Without this the frontend re-queued
    // every non-Steam entry through `resolve_covers` on *every* refresh, even
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
    log::info!("library ready: {} entries in {:?}", games.len(), started.elapsed());
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
        log::warn!("could not write {LIBRARY_CACHE_FILE}: {e}");
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
///
/// A remote URL is downloaded into `user_covers/` first and the local path is
/// what gets stored and returned: the CSP only lets the webview load images
/// from the IGDB CDN, so a pasted URL cannot be rendered directly.
#[tauri::command(async)]
async fn set_cover(app: AppHandle, id: String, url: Option<String>) -> Result<Option<String>, String> {
    blocking(move || {
        let value = match url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) if art::is_remote(u) => Some(art::fetch_user_cover(&app, &id, u)?),
            Some(u) => Some(u.to_string()),
            None => None,
        };
        storage::set_cover_override(&app, &id, value.as_deref())?;
        Ok(value)
    })
    .await
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

/// Threads one `resolve_covers` call may use. Equal to IGDB's in-flight cap
/// (`igdb::MAX_INFLIGHT`): more would only queue on that semaphore.
const COVER_WORKERS: usize = 4;
/// Threads one `app_icons` call may use (disk reads plus a PE parse each).
const ICON_WORKERS: usize = 4;

/// Resolve the grid covers for a list of game names via IGDB, cached on disk. One
/// answer per name, in the order asked.
///
/// A batch, not one command per game: a first scan used to cost one IPC round trip
/// per entry without artwork. The answer is tri-state (`art::Cover`) so the
/// frontend can tell "there is no cover" from "could not ask" and only remember the
/// first.
#[tauri::command(async)]
async fn resolve_covers(app: AppHandle, names: Vec<String>) -> Result<Vec<art::Cover>, String> {
    batch::check_len(names.len())?;
    blocking(move || {
        let answers = batch::map_ordered(&names, COVER_WORKERS, |name| art::resolve(&app, name));
        Ok(answers
            .into_iter()
            .map(|answer| answer.unwrap_or(art::Cover::Unavailable))
            .collect())
    })
    .await
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

/// A remote cover URL is downloaded into `user_covers/` (see `set_cover`).
#[tauri::command(async)]
async fn add_manual_app(
    app: AppHandle,
    name: String,
    executable: String,
    cover_url: Option<String>,
) -> Result<Game, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }

    blocking(move || {
        let mut manual = storage::load_manual(&app)?;
        let id = format!(
            "manual:{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis()
        );

        let cover_url = match cover_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(u) if art::is_remote(u) => Some(art::fetch_user_cover(&app, &id, u)?),
            Some(u) => Some(u.to_string()),
            None => None,
        };

        let game = Game {
            id,
            name,
            source: GameSource::Manual,
            app_id: None,
            executable: Some(executable),
            install_dir: None,
            cover_url,
            launch_uri: None,
            favorite: false,
            categories: Vec::new(),
        };

        manual.push(game.clone());
        storage::save_manual(&app, &manual)?;
        Ok(game)
    })
    .await
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

/// What the Steam client recorded as playtime for a Steam entry: minutes across
/// every machine of the account, as of the client's last write. Shown next to
/// Astrail's own measurement, never added to it. `None` = not a Steam entry, no
/// Steam account file, or Steam has no time for it.
///
/// Takes an id: the app id comes from our own library record, not the webview.
#[tauri::command(async)]
async fn steam_playtime(
    app: AppHandle,
    id: String,
) -> Result<Option<steam_playtime::SteamPlaytime>, String> {
    blocking(move || {
        let Some(game) = resolve_game(&app, &id) else {
            return Err(format!("Unknown entry: {id}"));
        };
        if game.source != GameSource::Steam {
            return Ok(None);
        }
        Ok(game.app_id.and_then(steam_playtime::for_app))
    })
    .await
}

/// The file to read an icon from for each id asked, in order. `None` for an id
/// that is not in the library or has no executable. The manual store wins over the
/// cache, as in `resolve_game`.
fn icon_sources(ids: &[String], manual: &[Game], cached: &[Game]) -> Vec<Option<String>> {
    ids.iter()
        .map(|id| {
            manual
                .iter()
                .chain(cached)
                .find(|game| &game.id == id)
                .and_then(|game| game.executable.clone())
                .filter(|exe| !exe.trim().is_empty())
        })
        .collect()
}

/// Extract the real icon embedded in each entry's executable (cached `.ico` path),
/// used for apps without a cover or known brand logo. One answer per id, in the
/// order asked.
///
/// Takes ids, never paths: this used to be `app_icon(path)`, which parsed whatever
/// file the webview named and added any `.ico` it named to the asset scope.
#[tauri::command(async)]
async fn app_icons(app: AppHandle, ids: Vec<String>) -> Result<Vec<Option<String>>, String> {
    batch::check_len(ids.len())?;
    blocking(move || {
        let manual = storage::load_manual(&app).unwrap_or_else(|e| {
            log::warn!("manual apps unreadable while resolving icons: {e}");
            Vec::new()
        });
        let sources = icon_sources(&ids, &manual, &read_library_cache(&app));
        let icons = batch::map_ordered(&sources, ICON_WORKERS, |source| {
            source.as_deref().and_then(|exe| appicons::extract(&app, exe))
        });
        Ok(icons.into_iter().map(Option::flatten).collect())
    })
    .await
}

/// Seconds the main window must stay hidden before we ask WebView2 to trim its
/// memory. Short enough to matter when Astrail lives in the tray, long enough not
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
fn show_main_window(app: AppHandle) {
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
static START_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a start keeps the main window hidden: only a logon start, only after
/// onboarding, and only when closing the window hides to the tray.
fn stays_in_tray(started_minimized: bool, minimize_to_tray: bool, setup_completed: bool) -> bool {
    started_minimized && minimize_to_tray && setup_completed
}

/// Main-window visibility, for a listener that registered after the first
/// `window-visibility` event was already sent.
// NOT `async`: a window getter, which belongs on the main thread.
#[tauri::command]
fn main_window_visible(app: AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(true)
}

/// Trim WebView2's memory once the main window has been hidden for a while,
/// and only if it is still hidden by then.
#[cfg(windows)]
fn schedule_webview_trim(app: AppHandle) {
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

/// Whether Astrail is set to launch on Windows login (the autostart `Run` key),
/// and whether this copy may turn it on (only an installed one may).
#[tauri::command(async)]
fn get_autostart() -> Result<autostart::AutostartState, String> {
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
fn set_autostart(enabled: bool) -> Result<(), String> {
    let result = if enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    result.map_err(|e| format!("Failed to update autostart: {e}"))
}

/// Lock the settings, recovering from a poisoned mutex.
///
/// Every reader of the settings runs on the main thread (commands, the close
/// handler, the hotkey handler). With `panic = "abort"` a plain `unwrap()` there
/// turns one earlier panic under the lock into the whole app dying on the next
/// window close or key press; the data under the lock is a plain struct that is
/// never left half-written, so the poisoned value is safe to keep using.
fn lock_settings(state: &std::sync::Mutex<AppSettings>) -> std::sync::MutexGuard<'_, AppSettings> {
    state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The settings in memory, or the stored ones before the state is managed.
fn current_settings(app: &AppHandle) -> AppSettings {
    app.try_state::<std::sync::Mutex<AppSettings>>()
        .map(|s| lock_settings(&s).clone())
        .unwrap_or_else(|| storage::load_settings(app))
}

#[tauri::command]
fn get_app_settings(state: tauri::State<'_, std::sync::Mutex<AppSettings>>) -> Result<AppSettings, String> {
    Ok(lock_settings(&state).clone())
}

#[tauri::command(async)]
fn system_info() -> Result<system::SystemInfo, String> {
    Ok(system::collect())
}

/// Version, author and license of this build, for Settings → About.
#[tauri::command]
fn about_info() -> about::AboutInfo {
    about::info()
}

/// The full text of one of the embedded legal documents.
// Async: the notices are half a megabyte, and serializing them should not sit
// on the main thread.
#[tauri::command(async)]
fn legal_document(name: String) -> Result<&'static str, String> {
    about::document(&name)
}

/// Overlay MPO diagnostics: live composition health + the system-config levers
/// (monitor count, mixed refresh, HAGS) that decide whether the HUD can run on a
/// hardware overlay plane (free) or gets composited by DWM (costing the game's FPS).
#[tauri::command(async)]
fn overlay_mpo_diagnostics() -> system::MpoDiagnostics {
    system::mpo_diagnostics()
}

/// Write one text file with everything needed to look into a bug report (build,
/// hardware, overlay/MPO state, settings, crash reports, the end of the log) and
/// show it in the file manager. Nothing leaves the machine: the user reads it and
/// decides whether to attach it. Returns the path.
#[tauri::command]
async fn export_diagnostics(app: AppHandle) -> Result<String, String> {
    blocking(move || {
        let settings = current_settings(&app);
        let header = diagnostics_header(&settings);
        let path = applog::write_diagnostics(&header)?;
        log::info!("diagnostics exported to {}", path.display());
        if let Some(dir) = path.parent() {
            files::open_folder(dir)?;
        }
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

fn diagnostics_header(settings: &AppSettings) -> String {
    fn json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("<not serializable: {e}>"))
    }
    format!(
        "Astrail {} diagnostics\ntime: {}\nmetrics access: {:?}\n\n===== system =====\n{}\n\n===== overlay / MPO =====\n{}\n\n===== settings =====\n{}\n",
        env!("CARGO_PKG_VERSION"),
        applog::timestamp(
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
        ),
        metrics_access(),
        json(&system::collect()),
        json(&system::mpo_diagnostics()),
        json(settings),
    )
}

/// The backup the user picked and saw the summary of, until they confirm or cancel.
///
/// The path and the hash stay on this side: the webview is never trusted with a
/// file to read or write (both dialogs are opened from Rust), and the restore
/// refuses a file whose bytes are not the ones the summary was made from.
#[derive(Default)]
struct PendingImport(std::sync::Mutex<Option<(std::path::PathBuf, [u8; 32])>>);

impl PendingImport {
    fn replace(&self, value: Option<(std::path::PathBuf, [u8; 32])>) -> Option<(std::path::PathBuf, [u8; 32])> {
        std::mem::replace(&mut *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner), value)
    }
}

/// A file dialog owned by the main window. Blocking: only call it from `blocking`.
fn backup_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().add_filter("Astrail backup", &["json"]);
    match app.get_webview_window("main") {
        Some(window) => dialog.set_parent(&window),
        None => dialog,
    }
}

/// Save the data only the user can recreate (manual apps, play time, favorites,
/// categories, hidden entries, chosen covers, settings) to a file they pick.
/// `None` = they closed the dialog. See `backup.rs` for what is in the file.
#[tauri::command]
async fn export_user_data(app: AppHandle) -> Result<Option<backup::ExportReport>, String> {
    blocking(move || {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        // `YYYY-MM-DD`: the timestamp is ASCII, so the slice cannot split a char.
        let day = applog::timestamp(now.as_millis()).get(..10).unwrap_or("data").to_string();
        let Some(target) = backup_dialog(&app)
            .set_file_name(format!("astrail-backup-{day}.json"))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let target = target.into_path().map_err(|e| format!("Unusable backup path: {e}"))?;
        let report = backup::export(&jsonstore::data_dir(&app)?, &target, now.as_secs())?;
        log::info!(
            "user data exported to {} ({} cover(s), {} skipped, {} unreadable store(s))",
            target.display(),
            report.covers,
            report.covers_skipped,
            report.unreadable.len()
        );
        Ok(Some(report))
    })
    .await
}

/// Let the user pick a backup and validate it. Nothing is written: the summary is
/// what they confirm, and `apply_user_data_backup` does the restore.
#[tauri::command]
async fn pick_user_data_backup(app: AppHandle) -> Result<Option<backup::ImportSummary>, String> {
    blocking(move || {
        let pending = app.state::<PendingImport>();
        pending.replace(None);
        // Start where the automatic and pre-import copies live, when it exists.
        let mut dialog = backup_dialog(&app);
        if let Ok(folder) = jsonstore::data_dir(&app).map(|d| d.join(backup::SAFETY_DIR)) {
            if folder.is_dir() {
                dialog = dialog.set_directory(folder);
            }
        }
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|e| format!("Unusable backup path: {e}"))?;
        let (restore, hash) = backup::read(&path)?;
        pending.replace(Some((path, hash)));
        Ok(Some(restore.summary))
    })
    .await
}

/// Forget the picked backup (the user cancelled, or closed the settings).
#[tauri::command]
fn discard_user_data_backup(pending: tauri::State<'_, PendingImport>) {
    pending.replace(None);
}

/// Replace the user's data with the backup picked in `pick_user_data_backup`. The
/// data being replaced is saved under `backups/` first; without that copy nothing
/// is touched.
#[tauri::command]
async fn apply_user_data_backup(app: AppHandle) -> Result<backup::ImportReport, String> {
    blocking(move || {
        let (path, hash) = app
            .state::<PendingImport>()
            .replace(None)
            .ok_or_else(|| "No backup is waiting to be imported".to_string())?;
        let restore = backup::read_verified(&path, &hash)?;
        let dir = jsonstore::data_dir(&app)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let safety = backup::write_safety_copy(&dir, now)
            .map_err(|e| format!("Nothing was imported: the current data could not be saved first ({e})"))?;
        let safety = safety.to_string_lossy().into_owned();

        let outcome = backup::apply(&dir, &restore).and_then(|covers| {
            restore_settings(&app, restore.settings.clone())?;
            Ok(covers)
        });
        if let Some(id) = restore.discord_id() {
            discord::set_client_id(id);
        }
        // Even after a failure: some stores may already hold the restored data.
        events::user_data_imported(&app);
        events::playtime_updated(&app, None);
        match outcome {
            Ok(covers) => {
                log::info!("user data imported from a backup ({covers} cover(s)); previous data in {safety}");
                Ok(backup::ImportReport { covers, safety_copy: safety })
            }
            Err(e) => {
                log::error!("user data import stopped half way: {e}; previous data in {safety}");
                Err(format!("{e}. The import is incomplete; your previous data is in {safety}"))
            }
        }
    })
    .await
}

/// Swap in the settings of a backup: same lock, same order as `patch_app_settings`.
fn restore_settings(app: &AppHandle, settings: Option<AppSettings>) -> Result<(), String> {
    let (Some(mut next), Some(state)) = (settings, app.try_state::<std::sync::Mutex<AppSettings>>()) else {
        return Ok(());
    };
    let previous = {
        let mut current = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // A backup made before the first-run setup was finished must not bring
        // the setup back on a machine that already went through it.
        next.setup_completed |= current.setup_completed;
        // Not `storage::save_settings`, which logs a failed write and carries on:
        // here the user is told whether their settings were restored.
        jsonstore::save_if_changed(app, storage::SETTINGS_FILE, &next)?;
        std::mem::replace(&mut *current, next.clone())
    };
    settings_changed(app, &previous, &next);
    Ok(())
}

/// A render error or an unhandled rejection caught in the webview. The webview is
/// not trusted: the text is flattened to one bounded line, and a burst is dropped
/// after the first few so a render loop cannot fill the log.
// Async: a log line is a file write.
#[tauri::command(async)]
fn report_frontend_error(window: tauri::Window, message: String) {
    use std::sync::atomic::{AtomicU32, Ordering};
    const MAX_PER_RUN: u32 = 50;
    static SEEN: AtomicU32 = AtomicU32::new(0);
    let seen = SEEN.fetch_add(1, Ordering::Relaxed);
    if seen < MAX_PER_RUN {
        log::error!(
            target: "astrail::webview",
            "[{}] {}",
            window.label(),
            applog::sanitize_frontend_message(&message)
        );
    } else if seen == MAX_PER_RUN {
        log::error!(target: "astrail::webview", "further webview errors are not logged in this run");
    }
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

/// What the privileged metrics can use in this process, for the settings screen.
///
/// Each field is a fact the UI turns into advice: FPS through PresentMon needs
/// `etw` (admin *or* Performance Log Users), CPU temperature needs `elevated`
/// *and* `pawnio`. Group membership is fixed at logon; PawnIO can be installed
/// while Astrail runs, so the screen re-asks each time it opens.
#[derive(serde::Serialize, Clone, Copy, Debug)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct MetricsAccess {
    pub elevated: bool,
    pub etw: bool,
    pub pawnio: bool,
}

/// Whether Astrail is running elevated (admin). False on non-Windows.
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

#[tauri::command(async)]
fn metrics_access() -> MetricsAccess {
    #[cfg(windows)]
    {
        MetricsAccess {
            elevated: elevation::is_elevated(),
            etw: elevation::can_trace_etw(),
            pawnio: cputemp::pawnio_installed(),
        }
    }
    #[cfg(not(windows))]
    {
        MetricsAccess { elevated: false, etw: false, pawnio: false }
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
    log::info!("update downloaded: stopping the sidecars before the installer runs");
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
    log::warn!("update install did not happen: sidecars allowed again");
    crate::metrics::set_sidecars_suspended(false);
}

/// Relaunch Astrail as administrator (UAC prompt), then exit this instance.
///
/// `async` through `blocking()`: `ShellExecuteW("runas")` only returns once the
/// UAC prompt is answered, and on the main thread that froze the window, the tray
/// and the hotkeys for as long as the prompt stayed open.
#[tauri::command]
async fn restart_as_admin(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        blocking(elevation::relaunch_elevated).await?;
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
        Err("Only available on Windows.".into())
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
fn settings_changed(app: &AppHandle, previous: &AppSettings, next: &AppSettings) {
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
    metrics::configure(&settings.overlay);
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
        events::settings_updated(app);
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
