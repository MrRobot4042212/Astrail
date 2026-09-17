// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { invoke } from '@tauri-apps/api/core';
import type { Game, Category, PlayStat, AppSettings, AppSettingsPatch, SystemInfo, MpoDiagnostics } from './types';

/** Unified library across every source (Steam, Epic, GOG, EA, Ubisoft, Xbox, manual). */
export const getLibrary = () => invoke<Game[]>('get_library');

/** Last computed library from disk (instant paint; empty if never scanned). */
export const cachedLibrary = () => invoke<Game[]>('cached_library');

/** Reveal the main window (created hidden) once the UI has painted. */
export const showMainWindow = () => invoke<void>('show_main_window');

/** Launch any library entry, by id.
 *  The backend re-resolves the entry from the library cache / manual store, so
 *  the executable and protocol URI it launches are never built by the webview. */
export const launchGame = (id: string) => invoke<void>('launch_game', { id });

/** Whether any installed-games source changed since the last scan (cheap probe:
 *  store folder / registry timestamps, no scanners, no PowerShell). */
export const libraryChanged = () => invoke<boolean>('library_changed');

/** High-resolution cover for the detail hero (reuses the cached IGDB image id). */
export const resolveCoverHires = (name: string) =>
  invoke<string | null>('resolve_cover_hires', { name });

/** Resolve a cover image URL for a game name (SteamGridDB → Steam CDN, cached). */
export const resolveCover = (name: string) =>
  invoke<string | null>('resolve_cover', { name });

/** Set (or clear) a cover override. A remote URL is downloaded by Rust and the
 *  stored local path comes back, which is what the grid must render. */
export const setCover = (id: string, url: string | null) =>
  invoke<string | null>('set_cover', { id, url });

/** Save a dropped/picked local image as the cover. Returns the saved local path. */
export const setCoverImage = (id: string, data: number[], ext: string) =>
  invoke<string>('set_cover_image', { id, data, ext });

/** Wipe the cover cache (URLs + downloaded images) so everything re-resolves. */
export const clearCoverCache = () => invoke<void>('clear_cover_cache');

/** Hide a game from the library (false positives from the registry scan). */
export const hideGame = (id: string) => invoke<void>('hide_game', { id });

/** Unhide a specific game from the library. */
export const unhideGame = (id: string) => invoke<void>('unhide_game', { id });

/** Fetch the cached list of hidden games to show them in the UI. */
export const getHiddenLibrary = () => invoke<Game[]>('get_hidden_library');

/** How many games are currently hidden. */
export const hiddenCount = () => invoke<number>('hidden_count');

/** Restore every hidden game. */
export const restoreHidden = () => invoke<void>('restore_hidden');

/** Reclassify an entry as 'app' or 'game' (any other value clears the override). */
export const setGameType = (id: string, kind: 'app' | 'game' | '') =>
  invoke<void>('set_game_type', { id, kind });

/** Add a manual app. `coverUrl` maps to the Rust `cover_url` argument. */
export const addManualApp = (name: string, executable: string, coverUrl?: string) =>
  invoke<Game>('add_manual_app', { name, executable, coverUrl: coverUrl ?? null });

/** Remove a manual app by id (store entries are ignored server-side). */
export const removeGame = (id: string) => invoke<void>('remove_game', { id });

/** Mark or unmark a game as favorite (works for any source). */
export const setFavorite = (id: string, favorite: boolean) =>
  invoke<void>('set_favorite', { id, favorite });

/** Replace the manual category list for a game id (empty array clears it). */
export const setCategories = (id: string, categories: string[]) =>
  invoke<void>('set_categories', { id, categories });

/** Every explicitly-created category with its icon (persists with zero games). */
export const listCategories = () => invoke<Category[]>('list_categories');

/** Create a category by name, optionally with a bundled-icon key. */
export const addCategory = (name: string, icon?: string | null) =>
  invoke<void>('add_category', { name, icon: icon ?? null });

/** Set (or clear, with null) the icon key for an existing category. */
export const setCategoryIcon = (name: string, icon: string | null) =>
  invoke<void>('set_category_icon', { name, icon });

/** Rename a category everywhere (merges if the new name already exists). */
export const renameCategory = (oldName: string, newName: string) =>
  invoke<void>('rename_category', { old: oldName, new: newName });

/** Delete a category and strip it from every game. */
export const removeCategory = (name: string) =>
  invoke<void>('remove_category', { name });

/** Persist the explicit category order (as shown in the sidebar). */
export const setCategoryOrder = (names: string[]) =>
  invoke<void>('set_category_order', { names });

/** Accumulated play stats (seconds + last played) for a game id. */
export const getPlaytime = (id: string) => invoke<PlayStat>('get_playtime', { id });

/** Play stats for every tracked game id (for sorting the library). */
export const allPlaytime = () =>
  invoke<Record<string, PlayStat>>('all_playtime');

/** Size in bytes of a library entry install folder (null when it has none). */
export const gameDirSize = (id: string) =>
  invoke<number | null>('game_dir_size', { id });

/** Extract the real icon embedded in an app's executable (cached local .ico path). */
export const appIcon = (path: string) =>
  invoke<string | null>('app_icon', { path });

/** The saved Discord Rich Presence client id ('' = disabled). */
export const getDiscordClientId = () => invoke<string>('get_discord_client_id');

/** Save the Discord client id (applied live to the presence watcher). */
export const setDiscordClientId = (id: string) =>
  invoke<void>('set_discord_client_id', { id });

/** Whether Meteor launches on Windows login. */
export const getAutostart = () => invoke<boolean>('get_autostart');

/** Enable/disable launching Meteor on Windows login. */
export const setAutostart = (enabled: boolean) =>
  invoke<void>('set_autostart', { enabled });

/** Open an allowlisted community link in the user's browser (validated in Rust,
 *  so it never navigates the app's own webview away). */
export const openExternal = (url: string) => invoke<void>('open_external', { url });

/** Reveal a library entry folder in the OS file manager (validated in Rust). */
export const openGameFolder = (id: string) =>
  invoke<void>('open_game_folder', { id });

/** The user's own screenshots for a game (Steam + Windows Game Bar). Local paths. */
export const userScreenshots = (id: string) =>
  invoke<string[]>('user_screenshots', { id });

/** Get application settings. */
export const getAppSettings = () => invoke<AppSettings>('get_app_settings');

/** Change only the given settings. The merge happens in Rust under the settings
 *  lock, so two windows saving at once cannot overwrite each other. */
export const patchAppSettings = (patch: AppSettingsPatch) =>
  invoke<void>('patch_app_settings', { patch });

/** Hardware/system info for the "Mi equipo" settings panel. */
export const systemInfo = () => invoke<SystemInfo>('system_info');

/** Overlay MPO diagnostics: live health + config levers (monitors, refresh, HAGS). */
export const overlayMpoDiagnostics = () =>
  invoke<MpoDiagnostics>('overlay_mpo_diagnostics');

/** The OS user's display/full name (or login name) for the Home greeting. */
export const username = () => invoke<string>('username');

/** Whether Meteor runs elevated (admin) — needed for CPU temp / NVIDIA FPS. */
export const isElevated = () => invoke<boolean>('is_elevated');

/** Relaunch Meteor as administrator (UAC), then the current instance exits. */
export const restartAsAdmin = () => invoke<void>('restart_as_admin');

/** Stop the metrics sidecars before the updater installs (its install exits the
 *  process without the normal shutdown). Pair with `abortUpdate` if install fails. */
export const prepareForUpdate = () => invoke<void>('prepare_for_update');

/** Resume the sidecars after a failed install. */
export const abortUpdate = () => invoke<void>('abort_update');

/** Open/close the in-game overlay settings screen (shows/hides the WebView2 window
 *  and pauses the native HUD while open). */
export const setOverlayInteractive = (interactive: boolean) =>
  invoke<void>('set_overlay_interactive', { interactive });
