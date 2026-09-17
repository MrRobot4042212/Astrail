// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! One JSON file per concern in the app data dir.
//!
//! All reads and writes go through `jsonstore`, which makes every write atomic
//! (temp + rename) and every read distinguish "missing" from "corrupt" — a
//! corrupt store is quarantined instead of being silently replaced by defaults
//! and then overwritten. Never add a bare `fs::write` here.

use crate::jsonstore::{self, Loaded};
use crate::models::{AppSettings, Category, Game};
use std::collections::HashMap;
use std::fs;

const STORE_FILE: &str = "manual_apps.json";
const OVERRIDES_FILE: &str = "cover_overrides.json";
const HIDDEN_FILE: &str = "hidden.json";
const HIDDEN_CACHE_FILE: &str = "hidden_cache.json";
const FAVORITES_FILE: &str = "favorites.json";
const CATEGORIES_FILE: &str = "categories.json";
const CATEGORY_NAMES_FILE: &str = "category_names.json";
const CATEGORY_ICONS_FILE: &str = "category_icons.json";
const DISCORD_FILE: &str = "discord.json";
/// User overrides for an entry's kind: id → "app" | "game". Lets the user fix a
/// mis-classified item (a game detected as an app, or vice versa).
const TYPE_OVERRIDES_FILE: &str = "type_overrides.json";
const SETTINGS_FILE: &str = "app_settings.json";

use tauri::AppHandle;

/// Load manually-added apps. Returns an empty list if nothing is stored yet.
///
/// Corrupt content is reported, not swallowed: manual apps are the one store the
/// user cannot rebuild by rescanning, so the caller should surface the error.
pub fn load_manual(app: &AppHandle) -> Result<Vec<Game>, String> {
    match jsonstore::load::<Vec<Game>>(app, STORE_FILE) {
        Loaded::Present(games) => Ok(games),
        Loaded::Missing => Ok(Vec::new()),
        Loaded::Corrupt(e) => Err(format!("manual_apps.json corrupto: {e}")),
    }
}

/// Persist the full list of manually-added apps.
pub fn save_manual(app: &AppHandle, games: &[Game]) -> Result<(), String> {
    jsonstore::save(app, STORE_FILE, &games)
}

/// The Discord application client id for Rich Presence (empty = disabled).
pub fn load_discord_client_id(app: &AppHandle) -> String {
    jsonstore::load_or_default::<String>(app, DISCORD_FILE)
}

/// Persist the Discord client id (trimmed).
pub fn save_discord_client_id(app: &AppHandle, id: &str) -> Result<(), String> {
    jsonstore::save(app, DISCORD_FILE, &id.trim())
}

/// Allowed image extensions for a user-supplied (dropped/picked) cover.
const COVER_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// Save a user-supplied cover image (dropped or picked from disk) into the
/// `user_covers` dir under app data, replacing any previous one for this id, and
/// return its absolute path. The dir is in the asset-protocol scope so the
/// webview can render it; it survives `clear_cover_cache` (which only wipes the
/// auto-resolved `covers/` dir).
pub fn save_cover_image(
    app: &AppHandle,
    id: &str,
    data: &[u8],
    ext: &str,
) -> Result<String, String> {
    if data.is_empty() {
        return Err("La imagen está vacía".into());
    }
    let ext = ext.trim().trim_start_matches('.').to_lowercase();
    let ext = if COVER_EXTS.contains(&ext.as_str()) {
        ext
    } else {
        "jpg".to_string()
    };

    let dir = jsonstore::data_dir(app)?.join("user_covers");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let stem = crate::art::cache_key(id);

    // Drop any previous cover for this id (possibly a different extension).
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&stem) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let path = dir.join(format!("{stem}.{ext}"));
    jsonstore::write_atomic(&path, data)?;
    Ok(path.to_string_lossy().to_string())
}

/// User-set cover overrides keyed by game id. These win over auto-resolution, so
/// a cover can always be fixed by hand. Returns an empty map if none are stored.
pub fn load_cover_overrides(app: &AppHandle) -> HashMap<String, String> {
    jsonstore::load_or_default(app, OVERRIDES_FILE)
}

/// Set (or, with an empty url, clear) the cover override for a game id.
pub fn set_cover_override(app: &AppHandle, id: &str, url: Option<&str>) -> Result<(), String> {
    let mut map = load_cover_overrides(app);
    match url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => {
            map.insert(id.to_string(), u.to_string());
        }
        None => {
            map.remove(id);
        }
    }
    jsonstore::save(app, OVERRIDES_FILE, &map)
}

/// Ids of games the user has hidden from the library (mostly false positives
/// from the generic registry scan). Returns an empty list if none.
pub fn load_hidden(app: &AppHandle) -> Vec<String> {
    jsonstore::load_or_default(app, HIDDEN_FILE)
}

/// Hide or unhide a game id.
pub fn set_hidden(app: &AppHandle, id: &str, hidden: bool) -> Result<(), String> {
    let mut ids = load_hidden(app);
    if hidden {
        if !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    } else {
        ids.retain(|x| x != id);
    }
    jsonstore::save(app, HIDDEN_FILE, &ids)
}

/// Unhide everything.
pub fn clear_hidden(app: &AppHandle) -> Result<(), String> {
    jsonstore::remove(app, HIDDEN_FILE)
}

/// Save the cached metadata of hidden games, so the UI can show a list to unhide.
///
/// Only writes when the content actually changed: this runs on **every** library
/// scan, and for the common case (nothing hidden) it used to rewrite `[]` each
/// time.
pub fn save_hidden_cache(app: &AppHandle, games: &[Game]) -> Result<(), String> {
    jsonstore::save_if_changed(app, HIDDEN_CACHE_FILE, &games).map(|_| ())
}

/// Load the cached metadata of hidden games.
pub fn load_hidden_cache(app: &AppHandle) -> Result<Vec<Game>, String> {
    Ok(jsonstore::load_or_default(app, HIDDEN_CACHE_FILE))
}

/// Ids the user marked as favorites. Applied as an overlay in `get_library`.
/// Returns an empty list if none.
pub fn load_favorites(app: &AppHandle) -> Vec<String> {
    jsonstore::load_or_default(app, FAVORITES_FILE)
}

/// Mark or unmark a game id as favorite.
pub fn set_favorite(app: &AppHandle, id: &str, favorite: bool) -> Result<(), String> {
    let mut ids = load_favorites(app);
    if favorite {
        if !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    } else {
        ids.retain(|x| x != id);
    }
    jsonstore::save(app, FAVORITES_FILE, &ids)
}

/// User overrides for an entry's kind (id → "app" | "game"), applied as an
/// overlay in `get_library`. Returns an empty map if none are stored.
pub fn load_type_overrides(app: &AppHandle) -> HashMap<String, String> {
    jsonstore::load_or_default(app, TYPE_OVERRIDES_FILE)
}

/// Set (or clear, with `None`/`""`) the kind override for a game id. Accepts only
/// "app" or "game"; anything else clears the override (back to auto-detection).
pub fn set_type_override(app: &AppHandle, id: &str, kind: Option<&str>) -> Result<(), String> {
    let mut map = load_type_overrides(app);
    match kind {
        Some("app") => {
            map.insert(id.to_string(), "app".to_string());
        }
        Some("game") => {
            map.insert(id.to_string(), "game".to_string());
        }
        _ => {
            map.remove(id);
        }
    }
    jsonstore::save(app, TYPE_OVERRIDES_FILE, &map)
}

/// Global settings, falling back to defaults on a first run.
pub fn load_settings(app: &AppHandle) -> AppSettings {
    match jsonstore::load::<AppSettings>(app, SETTINGS_FILE) {
        Loaded::Present(mut settings) => {
            // Read-time migration: no write here, so a settings file that is never
            // saved again still stops stealing F9/F10/F11 from every application.
            settings.shortcuts.migrate_legacy_defaults();
            settings.overlay.migrate_legacy_gpu();
            settings
        }
        _ => AppSettings {
            setup_completed: false,
            minimize_to_tray: true,
            overlay: Default::default(),
            shortcuts: Default::default(),
            language: "system".to_string(),
            discord_enabled: false,
        },
    }
}

/// Persist settings. Skips the write when nothing changed — the overlay hotkey
/// goes through here on every press.
pub fn save_settings(app: &AppHandle, settings: &AppSettings) {
    if let Err(e) = jsonstore::save_if_changed(app, SETTINGS_FILE, settings) {
        eprintln!("[storage] could not save {SETTINGS_FILE}: {e}");
    }
}

/// User-assigned categories keyed by game id. Applied as an overlay in
/// `get_library`. Returns an empty map if none are stored.
pub fn load_categories(app: &AppHandle) -> HashMap<String, Vec<String>> {
    jsonstore::load_or_default(app, CATEGORIES_FILE)
}

/// Replace the full category list for a game id (an empty list clears the entry).
/// Names are trimmed and de-duplicated, preserving order.
pub fn set_categories(app: &AppHandle, id: &str, categories: &[String]) -> Result<(), String> {
    let mut clean: Vec<String> = Vec::new();
    for name in categories {
        let name = name.trim();
        if !name.is_empty() && !clean.iter().any(|c| c.eq_ignore_ascii_case(name)) {
            clean.push(name.to_string());
        }
    }
    let mut map = load_categories(app);
    if clean.is_empty() {
        map.remove(id);
    } else {
        map.insert(id.to_string(), clean);
    }
    jsonstore::save(app, CATEGORIES_FILE, &map)
}

/// Explicitly-created category names. These persist even with zero games, so a
/// category can be created from the sidebar and used to assign games afterwards.
pub fn load_category_names(app: &AppHandle) -> Vec<String> {
    jsonstore::load_or_default(app, CATEGORY_NAMES_FILE)
}

/// Icon key chosen for each category (resolved to an SVG on the frontend).
pub fn load_category_icons(app: &AppHandle) -> HashMap<String, String> {
    jsonstore::load_or_default(app, CATEGORY_ICONS_FILE)
}

/// Set (or, with None/empty, clear) the icon key for a category name.
pub fn set_category_icon(app: &AppHandle, name: &str, icon: Option<&str>) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }
    let mut map = load_category_icons(app);
    match icon.map(str::trim).filter(|i| !i.is_empty()) {
        Some(i) => {
            map.insert(name.to_string(), i.to_string());
        }
        None => {
            map.remove(name);
        }
    }
    jsonstore::save(app, CATEGORY_ICONS_FILE, &map)
}

/// Every explicitly-created category with its icon (zips names + icon map).
pub fn load_categories_meta(app: &AppHandle) -> Vec<Category> {
    let icons = load_category_icons(app);
    load_category_names(app)
        .into_iter()
        .map(|name| {
            let icon = icons.get(&name).cloned();
            Category { name, icon }
        })
        .collect()
}

/// Create a category by name (trimmed, deduped case-insensitively), optionally
/// with an icon key.
pub fn add_category_name(app: &AppHandle, name: &str, icon: Option<&str>) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }
    let mut names = load_category_names(app);
    if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        names.push(name.to_string());
    }
    jsonstore::save(app, CATEGORY_NAMES_FILE, &names)?;
    if icon.map(str::trim).is_some_and(|i| !i.is_empty()) {
        set_category_icon(app, name, icon)?;
    }
    Ok(())
}

/// Persist the explicit category order. Trims, drops empties and de-dupes
/// (case-insensitive, first wins). Promotes any in-use names passed in to
/// explicit categories so the chosen order sticks.
pub fn set_category_order(app: &AppHandle, names: &[String]) -> Result<(), String> {
    let mut clean: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim();
        if !n.is_empty() && !clean.iter().any(|c| c.eq_ignore_ascii_case(n)) {
            clean.push(n.to_string());
        }
    }
    jsonstore::save(app, CATEGORY_NAMES_FILE, &clean)
}

/// Rename a category everywhere: the names list (keeping its position), its icon,
/// and every game's assigned list. If `new` already exists the two **merge**.
pub fn rename_category_name(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    let new = new.trim();
    if new.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }

    // 1. Names list — replace old with new in place, normalizing/merging.
    let names = load_category_names(app);
    let new_preexists = names
        .iter()
        .any(|n| n.eq_ignore_ascii_case(new) && !n.eq_ignore_ascii_case(old));
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for n in names {
        if n.eq_ignore_ascii_case(old) {
            if !new_preexists && !placed {
                out.push(new.to_string());
                placed = true;
            }
            // merging into an existing target → drop the old entry
        } else if n.eq_ignore_ascii_case(new) {
            if !placed {
                out.push(new.to_string());
                placed = true;
            }
        } else {
            out.push(n);
        }
    }
    if !placed {
        out.push(new.to_string()); // old was in-use only → make it explicit
    }
    jsonstore::save(app, CATEGORY_NAMES_FILE, &out)?;

    // 2. Icon — carry old's icon over to new if new doesn't have one already.
    let mut icons = load_category_icons(app);
    if let Some(icon) = icons.remove(old) {
        icons.entry(new.to_string()).or_insert(icon);
    }
    jsonstore::save(app, CATEGORY_ICONS_FILE, &icons)?;

    // 3. Every game's list — old → new, de-duplicated case-insensitively.
    let mut map = load_categories(app);
    for cats in map.values_mut() {
        let mut nc: Vec<String> = Vec::new();
        for c in cats.drain(..) {
            let name = if c.eq_ignore_ascii_case(old) {
                new.to_string()
            } else {
                c
            };
            if !nc.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                nc.push(name);
            }
        }
        *cats = nc;
    }
    jsonstore::save(app, CATEGORIES_FILE, &map)
}

/// Delete a category: remove the name, its icon, and strip it from every game.
pub fn remove_category_name(app: &AppHandle, name: &str) -> Result<(), String> {
    let mut names = load_category_names(app);
    names.retain(|n| !n.eq_ignore_ascii_case(name));
    jsonstore::save(app, CATEGORY_NAMES_FILE, &names)?;

    set_category_icon(app, name, None)?;

    let mut map = load_categories(app);
    for cats in map.values_mut() {
        cats.retain(|c| !c.eq_ignore_ascii_case(name));
    }
    map.retain(|_, v| !v.is_empty());
    jsonstore::save(app, CATEGORIES_FILE, &map)
}

/// One-time rename of `user_covers/<DefaultHasher>.<ext>` to the FNV-1a key.
///
/// Unlike downloaded art, a user cover cannot be re-fetched, so the rename is
/// driven by `cover_overrides.json` (which stores each file's absolute path)
/// rather than by recomputing names — and the override is updated to point at
/// the new file, so a failed rename leaves everything working as before.
pub fn migrate_user_cover_filenames(app: &AppHandle) {
    let Ok(dir) = jsonstore::data_dir(app).map(|d| d.join("user_covers")) else {
        return;
    };
    if !dir.exists() {
        return;
    }
    let mut overrides = load_cover_overrides(app);
    let mut changed = false;
    for (id, path) in overrides.iter_mut() {
        let current = std::path::Path::new(path.as_str()).to_path_buf();
        if current.parent() != Some(dir.as_path()) {
            continue; // a remote URL or a file outside our folder
        }
        let Some(stem) = current.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem != crate::art::legacy_key(id) {
            continue; // already migrated (or never used the hashed scheme)
        }
        let ext = current
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_string();
        let target = dir.join(format!("{}.{ext}", crate::art::cache_key(id)));
        if current.exists() && !target.exists() && fs::rename(&current, &target).is_ok() {
            *path = target.to_string_lossy().to_string();
            changed = true;
        }
    }
    if changed {
        if let Err(e) = jsonstore::save(app, OVERRIDES_FILE, &overrides) {
            eprintln!("[storage] user cover migration could not be saved: {e}");
        }
    }
}
