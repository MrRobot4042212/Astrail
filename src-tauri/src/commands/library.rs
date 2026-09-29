// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! The library: scanning, the cache, covers and icons, user overlays
//! (favorites, categories, hidden entries, types), play time, launching and
//! opening a game's folder.

use crate::error::{AppError, CmdResult, ErrorCode};
use crate::models::{Category, Game, GameSource};
use crate::{
    appicons, art, batch, battlenet, blocking, ea, epic, events, files, fingerprint, gog, jsonstore,
    launcher, library, library_cache, perf, playtime, screenshots, steam, storage,
    ubisoft, windows_apps, xbox,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

/// Return the unified library across every supported source, sorted by name.
///
/// Each scanner runs independently: a failure in one source (store not
/// installed, corrupt manifest…) degrades to an empty list for that source
/// instead of failing the whole call. Sources are merged in priority order and
/// deduplicated by name, so a game owned on several stores shows up once with
/// the best available metadata (Steam first, since it ships CDN cover art).
#[tauri::command(async)]
pub(crate) async fn get_library(app: AppHandle) -> CmdResult<Vec<Game>> {
    blocking(move || get_library_inner(app)).await
}

/// A store that fails degrades to an empty list (scanners are best-effort), but
/// the reason is kept: "Steam is not installed" and "the library folders could not
/// be read" look the same from the grid.
pub(crate) fn scanned(store: &str, result: Result<Vec<Game>, String>) -> Vec<Game> {
    result.unwrap_or_else(|e| {
        log::info!("{store} scan returned nothing: {e}");
        Vec::new()
    })
}

pub(crate) fn get_library_inner(app: AppHandle) -> Result<Vec<Game>, String> {
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
    log::debug!("library: {} duplicate entries folded into another store's copy", aliases.len());

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
    library_cache::write(&app, &games);
    // Remember what the stores looked like, so `library_changed` can answer
    // without redoing any of this.
    let _ = jsonstore::save(&app, FINGERPRINT_FILE, &fingerprint);
    // The watcher re-reads the index when this file changes; nudge it so a game
    // launched right after a scan is picked up immediately.
    playtime::wake();
    log::info!("library ready: {} entries in {:?}", games.len(), started.elapsed());
    Ok(games)
}

/// Fingerprint of the installed-games sources at the last successful scan.
pub(crate) const FINGERPRINT_FILE: &str = "library_fingerprint.json";

/// Resolve a library entry **in Rust** from an id the frontend sent.
///
/// Commands take ids, never whole `Game` structs: a struct coming from the
/// webview is attacker-controlled input that would otherwise flow straight into
/// `ShellExecuteW`/`CreateProcess` (see `launcher.rs`). The manual store wins
/// over the cache because it is the authoritative record for `manual:` entries.
pub(crate) fn resolve_game(app: &AppHandle, id: &str) -> Option<Game> {
    if let Ok(manual) = storage::load_manual(app) {
        if let Some(game) = manual.into_iter().find(|g| g.id == id) {
            return Some(game);
        }
    }
    library_cache::read(app).into_iter().find(|g| g.id == id)
}

/// Folder to reveal for an entry: its install dir, or the executable parent.
pub(crate) fn game_folder(game: &Game) -> Option<String> {
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
pub(crate) fn cached_library(app: AppHandle) -> CmdResult<Vec<Game>> {
    Ok(library_cache::read(&app))
}

/// Set a manual cover URL for a game id (empty/None clears it). Overrides always
/// take precedence over auto-resolved artwork.
///
/// A remote URL is downloaded into `user_covers/` first and the local path is
/// what gets stored and returned: the CSP only lets the webview load images
/// from the IGDB CDN, so a pasted URL cannot be rendered directly.
#[tauri::command(async)]
pub(crate) async fn set_cover(app: AppHandle, id: String, url: Option<String>) -> CmdResult<Option<String>> {
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
pub(crate) fn set_cover_image(
    app: AppHandle,
    id: String,
    data: Vec<u8>,
    ext: String,
) -> CmdResult<String> {
    if data.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidInput, "the image is empty"));
    }
    let path = storage::save_cover_image(&app, &id, &data, &ext)?;
    storage::set_cover_override(&app, &id, Some(&path))?;
    Ok(path)
}

/// Threads one `resolve_covers` call may use. Equal to IGDB's in-flight cap
/// (`igdb::MAX_INFLIGHT`): more would only queue on that semaphore.
pub(crate) const COVER_WORKERS: usize = 4;
/// Threads one `app_icons` call may use (disk reads plus a PE parse each).
pub(crate) const ICON_WORKERS: usize = 4;

/// Resolve the grid covers for a list of game names via IGDB, cached on disk. One
/// answer per name, in the order asked.
///
/// A batch, not one command per game: a first scan used to cost one IPC round trip
/// per entry without artwork. The answer is tri-state (`art::Cover`) so the
/// frontend can tell "there is no cover" from "could not ask" and only remember the
/// first.
#[tauri::command(async)]
pub(crate) async fn resolve_covers(app: AppHandle, names: Vec<String>) -> CmdResult<Vec<art::Cover>> {
    batch::check_len(names.len())?;
    blocking(move || {
        let answers: Vec<art::Cover> = batch::map_ordered(&names, COVER_WORKERS, |name| art::resolve(&app, name))
            .into_iter()
            .map(|answer| answer.unwrap_or(art::Cover::Unavailable))
            .collect();
        let found = answers.iter().filter(|a| matches!(a, art::Cover::Found(_))).count();
        let unavailable = answers.iter().filter(|a| matches!(a, art::Cover::Unavailable)).count();
        log::debug!(
            "covers: {} asked, {found} found, {} not found, {unavailable} unavailable",
            answers.len(),
            answers.len() - found - unavailable
        );
        Ok(answers)
    })
    .await
}

/// Whether anything that feeds the library has changed since the last scan.
///
/// The frontend calls this before its periodic refresh: when nothing moved, the
/// whole 8-scanner + PowerShell pass is skipped. Conservative — any source it
/// cannot read counts as changed.
#[tauri::command(async)]
pub(crate) async fn library_changed(app: AppHandle) -> CmdResult<bool> {
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
pub(crate) async fn resolve_cover_hires(app: AppHandle, name: String) -> CmdResult<Option<String>> {
    blocking(move || Ok(art::resolve_hires(&app, &name))).await
}

/// Wipe the cover cache (URLs + downloaded images) so everything re-resolves.
#[tauri::command(async)]
pub(crate) async fn clear_cover_cache(app: AppHandle) -> CmdResult<()> {
    blocking(move || art::clear_cache(&app)).await
}

/// Reclassify an entry as an application or a game (`"app"` / `"game"`), or clear
/// the override (any other value) to fall back to auto-detection.
#[tauri::command(async)]
pub(crate) fn set_game_type(app: AppHandle, id: String, kind: String) -> CmdResult<()> {
    let k = kind.as_str();
    storage::set_type_override(&app, &id, if k == "app" || k == "game" { Some(k) } else { None }).map_err(AppError::with(ErrorCode::Io))
}

/// Hide a game from the library (e.g. a non-game picked up by the registry scan).
#[tauri::command(async)]
pub(crate) fn hide_game(app: AppHandle, id: String) -> CmdResult<()> {
    storage::set_hidden(&app, &id, true).map_err(AppError::with(ErrorCode::Io))
}

/// Unhide a game from the library.
#[tauri::command(async)]
pub(crate) fn unhide_game(app: AppHandle, id: String) -> CmdResult<()> {
    storage::set_hidden(&app, &id, false).map_err(AppError::with(ErrorCode::Io))
}

/// Get the cached metadata of hidden games.
#[tauri::command(async)]
pub(crate) fn get_hidden_library(app: AppHandle) -> CmdResult<Vec<Game>> {
    storage::load_hidden_cache(&app).map_err(AppError::with(ErrorCode::Io))
}

/// Number of currently-hidden games (shown in settings so they can be restored).
#[tauri::command(async)]
pub(crate) fn hidden_count(app: AppHandle) -> CmdResult<usize> {
    Ok(storage::load_hidden(&app).len())
}

/// Restore every hidden game.
#[tauri::command(async)]
pub(crate) fn restore_hidden(app: AppHandle) -> CmdResult<()> {
    storage::clear_hidden(&app).map_err(AppError::with(ErrorCode::Io))
}

/// A remote cover URL is downloaded into `user_covers/` (see `set_cover`).
#[tauri::command(async)]
pub(crate) async fn add_manual_app(
    app: AppHandle,
    name: String,
    executable: String,
    cover_url: Option<String>,
) -> CmdResult<Game> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidInput, "the name cannot be empty"));
    }

    blocking(move || {
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

        // The cover is downloaded above, outside the store's lock.
        storage::update_manual(&app, |manual| manual.push(game.clone()))?;
        Ok(game)
    })
    .await
}

/// Remove a manually-added app. Store-managed entries are ignored.
#[tauri::command(async)]
pub(crate) fn remove_game(app: AppHandle, id: String) -> CmdResult<()> {
    storage::update_manual(&app, |manual| manual.retain(|g| g.id != id))?;
    Ok(())
}

/// Mark or unmark a game as favorite (applies to any source, not just manual).
#[tauri::command(async)]
pub(crate) fn set_favorite(app: AppHandle, id: String, favorite: bool) -> CmdResult<()> {
    storage::set_favorite(&app, &id, favorite).map_err(AppError::with(ErrorCode::Io))
}

/// Replace the manual category list for a game id (empty list clears it).
#[tauri::command(async)]
pub(crate) fn set_categories(app: AppHandle, id: String, categories: Vec<String>) -> CmdResult<()> {
    storage::set_categories(&app, &id, &categories).map_err(AppError::with(ErrorCode::Io))
}

/// Every explicitly-created category with its icon (persist even with zero games).
#[tauri::command(async)]
pub(crate) fn list_categories(app: AppHandle) -> CmdResult<Vec<Category>> {
    Ok(storage::load_categories_meta(&app))
}

/// Create a category by name, optionally with an icon key from the bundled set.
#[tauri::command(async)]
pub(crate) fn add_category(app: AppHandle, name: String, icon: Option<String>) -> CmdResult<()> {
    require_name(&name)?;
    storage::add_category_name(&app, &name, icon.as_deref()).map_err(AppError::with(ErrorCode::Io))
}

/// A category name the webview sent: blank is the user's mistake, not a disk
/// failure, so it is refused here with its own code (`storage` checks it again).
pub(crate) fn require_name(name: &str) -> CmdResult<()> {
    if name.trim().is_empty() {
        return Err(AppError::new(ErrorCode::InvalidInput, "the name cannot be empty"));
    }
    Ok(())
}

/// Set (or clear, with None) the icon key for an existing category.
#[tauri::command(async)]
pub(crate) fn set_category_icon(app: AppHandle, name: String, icon: Option<String>) -> CmdResult<()> {
    require_name(&name)?;
    storage::set_category_icon(&app, &name, icon.as_deref()).map_err(AppError::with(ErrorCode::Io))
}

/// Delete a category and strip it from every game.
#[tauri::command(async)]
pub(crate) fn remove_category(app: AppHandle, name: String) -> CmdResult<()> {
    storage::remove_category_name(&app, &name).map_err(AppError::with(ErrorCode::Io))
}

/// Rename a category everywhere (merges if the new name already exists).
#[tauri::command(async)]
pub(crate) fn rename_category(app: AppHandle, old: String, new: String) -> CmdResult<()> {
    require_name(&new)?;
    storage::rename_category_name(&app, &old, &new).map_err(AppError::with(ErrorCode::Io))
}

/// Persist the explicit category order (as shown in the sidebar).
#[tauri::command(async)]
pub(crate) fn set_category_order(app: AppHandle, names: Vec<String>) -> CmdResult<()> {
    storage::set_category_order(&app, &names).map_err(AppError::with(ErrorCode::Io))
}

/// Accumulated play stats (seconds + last played) for a game id.
#[tauri::command(async)]
pub(crate) fn get_playtime(app: AppHandle, id: String) -> CmdResult<playtime::PlayStat> {
    Ok(playtime::get(&app, &id))
}

/// Play stats for every tracked game id (for sorting the library).
#[tauri::command(async)]
pub(crate) fn all_playtime(
    app: AppHandle,
) -> CmdResult<std::collections::HashMap<String, playtime::PlayStat>> {
    Ok(playtime::all(&app))
}

/// Size on disk of a library entry install folder, for the detail page.
///
/// Takes an id, not a path: `install_dir` is read from our own library cache and
/// validated, so the webview cannot ask for the size of an arbitrary directory.
/// `None` = the entry has no known folder.
#[tauri::command(async)]
pub(crate) async fn game_dir_size(app: AppHandle, id: String) -> CmdResult<Option<u64>> {
    blocking(move || {
        let Some(game) = resolve_game(&app, &id) else {
            return Err(format!("unknown entry: {id}"));
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
pub(crate) async fn steam_playtime(
    app: AppHandle,
    id: String,
) -> CmdResult<Option<crate::steam_playtime::SteamPlaytime>> {
    blocking(move || {
        let Some(game) = resolve_game(&app, &id) else {
            return Err(format!("Unknown entry: {id}"));
        };
        if game.source != GameSource::Steam {
            return Ok(None);
        }
        Ok(game.app_id.and_then(crate::steam_playtime::for_app))
    })
    .await
}

/// The file to read an icon from for each id asked, in order. `None` for an id
/// that is not in the library or has no executable. The manual store wins over the
/// cache, as in `resolve_game`.
pub(crate) fn icon_sources(ids: &[String], manual: &[Game], cached: &[Game]) -> Vec<Option<String>> {
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
pub(crate) async fn app_icons(app: AppHandle, ids: Vec<String>) -> CmdResult<Vec<Option<String>>> {
    batch::check_len(ids.len())?;
    blocking(move || {
        let manual = storage::load_manual(&app).unwrap_or_else(|e| {
            log::warn!("manual apps unreadable while resolving icons: {e}");
            Vec::new()
        });
        let sources = icon_sources(&ids, &manual, &library_cache::read(&app));
        let icons = batch::map_ordered(&sources, ICON_WORKERS, |source| {
            source.as_deref().and_then(|exe| appicons::extract(&app, exe))
        });
        Ok(icons.into_iter().map(Option::flatten).collect())
    })
    .await
}

/// Reveal a library entry folder in the OS file manager.
///
/// Id-based for the same reason as `game_dir_size`: the old `open_path(path)`
/// handed an arbitrary webview-supplied string to Explorer.
#[tauri::command(async)]
pub(crate) fn open_game_folder(app: AppHandle, id: String) -> CmdResult<()> {
    let Some(game) = resolve_game(&app, &id) else {
        return Err(AppError::new(ErrorCode::NotFound, format!("unknown entry: {id}")));
    };
    let folder = game_folder(&game)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "the entry has no folder"))?;
    let dir = files::validate_dir(&folder).map_err(AppError::with(ErrorCode::NotFound))?;
    files::open_folder(&dir).map_err(AppError::with(ErrorCode::Io))
}

/// Open one of the detail page's community links in the user's browser.
///
/// Validated against a host allowlist in Rust: a bare `<a href>` in the webview
/// navigates the app window itself, and the target host must not be whatever a
/// library entry happens to contain.
#[tauri::command(async)]
pub(crate) fn open_external(url: String) -> CmdResult<()> {
    files::open_external(&url).map_err(AppError::with(ErrorCode::InvalidInput))
}

/// The user's own screenshots for a game (Steam + Windows Game Bar).
#[tauri::command(async)]
pub(crate) fn user_screenshots(app: AppHandle, id: String) -> CmdResult<Vec<String>> {
    let Some(game) = resolve_game(&app, &id) else {
        return Err(AppError::new(ErrorCode::NotFound, format!("unknown entry: {id}")));
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
pub(crate) async fn launch_game(app: AppHandle, id: String) -> CmdResult<()> {
    let resolver = app.clone();
    let game = blocking(move || {
        resolve_game(&resolver, &id).ok_or_else(|| format!("Unknown library entry: {id}"))
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::NotFound, e.detail))?;
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
    .map_err(|e| AppError::new(ErrorCode::LaunchFailed, e.detail))
}
