//! Extract the real icon embedded in an application's executable, so any
//! installed app shows its correct icon — no curated list needed.
//!
//! The exe's primary icon group is read straight from its PE resources (pure
//! Rust, no native libs) and written to a cached `.ico` in `app_icons/`, then
//! authorized in the asset-protocol scope at runtime so the webview can render
//! it via `convertFileSrc` (WebView2/Chromium displays `.ico` in `<img>`). Used
//! by the frontend as a fallback when an entry has neither a cover nor a known
//! brand logo.
//!
//! The cache follows the same rules as `covers/`: filenames are FNV-1a
//! (`art::cache_key`, stable across toolchains), files are written atomically,
//! and the directory is kept under `ICONS_MAX_BYTES` by `maintain`.

use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const ICONS_DIR: &str = "app_icons";

/// Marker written once the directory holds FNV-1a names only. Its absence means
/// the files were named by `DefaultHasher`, which cannot be mapped back without
/// the source paths — and an icon is a 100 KB local PE read, so they are simply
/// dropped and re-extracted on demand.
const NAMING_MARKER: &str = ".fnv1a";

/// Size cap for `app_icons/`. A typical icon group is 20–300 KB, so this holds
/// several hundred apps; beyond it the least recently used are dropped.
const ICONS_MAX_BYTES: u64 = 64 * 1024 * 1024;

fn cache_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join(ICONS_DIR);
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A `DisplayIcon`/exe value can carry an index (`C:\app\app.exe,0`) — keep the
/// path part only.
fn icon_source(raw: &str) -> String {
    raw.split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_string()
}

/// Cache filename stem for an executable path (case-insensitive, stable).
fn hashed(path: &str) -> String {
    crate::art::cache_key(&path.to_lowercase())
}

/// Extract (or reuse the cached) icon for an executable/icon path. Returns a
/// local `.ico` path authorized in the asset scope, or None if extraction fails.
#[cfg(windows)]
pub fn extract(app: &AppHandle, source: &str) -> Option<String> {
    use pelite::{FileMap, PeFile};

    let path = icon_source(source);
    if path.is_empty() || !std::path::Path::new(&path).exists() {
        return None;
    }

    // If the source is already an .ico, serve it directly.
    if path.to_lowercase().ends_with(".ico") {
        let _ = app.asset_protocol_scope().allow_file(&path);
        return Some(path);
    }

    let dir = cache_dir(app)?;
    let out = dir.join(format!("{}.ico", hashed(&path)));

    if !out.exists() {
        let map = FileMap::open(&path).ok()?;
        let file = PeFile::from_bytes(&map).ok()?;
        let resources = file.resources().ok()?;
        // The first icon group is the app's primary icon.
        let mut ico = Vec::new();
        let mut wrote = false;
        for (_, group) in resources.icons().filter_map(Result::ok) {
            if group.write(&mut ico).is_ok() {
                wrote = true;
                break;
            }
        }
        if !wrote {
            return None;
        }
        // Atomic: a half-written .ico would be served as a broken image forever.
        crate::jsonstore::write_atomic(&out, &ico).ok()?;
    }

    // Authorize every call (cached too) so it survives restarts.
    let _ = app.asset_protocol_scope().allow_file(&out);
    Some(out.to_string_lossy().to_string())
}

#[cfg(not(windows))]
pub fn extract(_app: &AppHandle, _source: &str) -> Option<String> {
    None
}

/// Startup maintenance, off the main thread: drop icons named by the old
/// unstable hash once, then keep the directory under its size cap.
pub fn maintain(app: &AppHandle) {
    let Some(dir) = cache_dir(app) else { return };
    let marker = dir.join(NAMING_MARKER);
    if !marker.exists() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(".ico") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let _ = fs::write(&marker, b"");
    }
    let freed = crate::art::prune_lru(&dir, ICONS_MAX_BYTES);
    if freed > 0 {
        eprintln!(
            "[appicons] pruned {} MB of cached icons (cap {} MB)",
            freed / (1024 * 1024),
            ICONS_MAX_BYTES / (1024 * 1024)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_filenames_use_the_stable_hash() {
        // Regression (A21): `DefaultHasher` names changed with the toolchain, so
        // every Rust bump orphaned the whole icon cache.
        assert_eq!(hashed(r"C:\Apps\Foo\foo.exe"), crate::art::cache_key(r"c:\apps\foo\foo.exe"));
        assert_eq!(hashed(r"C:\Apps\Foo\FOO.EXE"), hashed(r"c:\apps\foo\foo.exe"));
        assert_eq!(hashed("x").len(), 16);
    }

    #[test]
    fn the_icon_index_suffix_is_dropped_from_the_source() {
        assert_eq!(icon_source(r#""C:\app\app.exe",0"#), r"C:\app\app.exe");
        assert_eq!(icon_source(r"C:\app\app.exe"), r"C:\app\app.exe");
    }
}
