// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Atomic JSON persistence for every store in the app data dir.
//!
//! Meteor keeps one JSON file per concern (manual apps, favorites, categories,
//! settings, playtime…). Before this module each of them was a bare
//! `fs::write`, and every loader ended in `.ok().unwrap_or_default()`. Two
//! failure modes followed from that:
//!
//! 1. **Torn files.** A crash or power loss mid-write left a truncated JSON —
//!    and since the loader treated unparseable as "empty", the next save
//!    silently persisted the empty value. Data loss with no error anywhere.
//! 2. **Silent wipes.** The same path turned *any* corruption (a bad edit, a
//!    disk error) into "you have no favorites/categories/manual apps".
//!
//! So: writes go to `<file>.tmp`, are flushed to disk, and are then `rename`d
//! over the target (a single atomic replace on NTFS). Reads distinguish
//! *missing* from *corrupt*; corrupt files are **quarantined** as
//! `<file>.corrupt-<unix-ts>` instead of being overwritten, and if that
//! quarantine fails the file is marked poisoned and further saves to it are
//! refused rather than destroying the user's data.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

/// Outcome of reading a store.
#[derive(Debug)]
pub enum Loaded<T> {
    Present(T),
    /// The file does not exist yet — a first run, not an error.
    Missing,
    /// The file exists but could not be parsed. Already quarantined (or, if the
    /// quarantine failed, poisoned — see `save`).
    Corrupt(String),
}

impl<T> Loaded<T> {
    /// The value, or `T::default()` for both missing and corrupt.
    pub fn or_default(self) -> T
    where
        T: Default,
    {
        match self {
            Loaded::Present(v) => v,
            _ => T::default(),
        }
    }
}

/// Files whose corrupt content could not be quarantined. Saving to one of these
/// would destroy data we failed to back up, so `save` refuses.
static POISONED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn poisoned() -> &'static Mutex<HashSet<String>> {
    POISONED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn is_poisoned(file: &str) -> bool {
    poisoned()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(file)
}

/// The app data dir, created once per process instead of on every access.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("No se pudo obtener la carpeta de datos: {e}"))?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    })
    .clone()
}

/// Absolute path of a store file inside the app data dir.
pub fn path(app: &AppHandle, file: &str) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join(file))
}

/// Move an unparseable file aside so the next save cannot overwrite it.
fn quarantine(path: &Path, file: &str, err: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = path.with_file_name(format!("{file}.corrupt-{ts}"));
    match fs::rename(path, &backup) {
        Ok(()) => eprintln!(
            "[storage] {file} is corrupt ({err}); moved to {} and starting from defaults",
            backup.display()
        ),
        Err(e) => {
            eprintln!(
                "[storage] {file} is corrupt ({err}) and could not be quarantined ({e}); refusing to overwrite it"
            );
            poisoned()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(file.to_string());
        }
    }
}

/// Read and parse a store file.
pub fn load<T: DeserializeOwned>(app: &AppHandle, file: &str) -> Loaded<T> {
    let path = match path(app, file) {
        Ok(p) => p,
        Err(e) => return Loaded::Corrupt(e),
    };
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(e) => return Loaded::Corrupt(e.to_string()),
    };
    // An empty file is what a torn write leaves behind; treat it as missing so a
    // first-run default is written back without a scary quarantine file.
    if data.trim().is_empty() {
        return Loaded::Missing;
    }
    match serde_json::from_str(&data) {
        Ok(value) => Loaded::Present(value),
        Err(e) => {
            quarantine(&path, file, &e.to_string());
            Loaded::Corrupt(e.to_string())
        }
    }
}

/// `load` with `T::default()` for missing/corrupt, for the many callers that
/// have nothing better to do than start empty.
pub fn load_or_default<T: DeserializeOwned + Default>(app: &AppHandle, file: &str) -> T {
    load::<T>(app, file).or_default()
}

/// Serialize and write atomically: temp file → flush → rename over the target.
pub fn save<T: Serialize>(app: &AppHandle, file: &str, value: &T) -> Result<(), String> {
    if is_poisoned(file) {
        return Err(format!(
            "{file} contiene datos corruptos que no se pudieron respaldar; no se sobrescribe"
        ));
    }
    let path = path(app, file)?;
    let data = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    write_atomic(&path, data.as_bytes())
}

/// Like `save`, but skips the write when the file already has these exact bytes.
/// Returns whether anything was written.
///
/// This is what keeps idle Meteor off the disk: the playtime watcher rewrote
/// `active_sessions.json` every 5 seconds with the same `[]`.
pub fn save_if_changed<T: Serialize>(
    app: &AppHandle,
    file: &str,
    value: &T,
) -> Result<bool, String> {
    if is_poisoned(file) {
        return Err(format!(
            "{file} contiene datos corruptos que no se pudieron respaldar; no se sobrescribe"
        ));
    }
    let path = path(app, file)?;
    let data = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    if let Ok(existing) = fs::read(&path) {
        if existing == data.as_bytes() {
            return Ok(false);
        }
    }
    write_atomic(&path, data.as_bytes())?;
    Ok(true)
}

/// Delete a store file (no-op if absent).
pub fn remove(app: &AppHandle, file: &str) -> Result<(), String> {
    let path = path(app, file)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Write bytes so that readers only ever see the old or the new content.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        // Without the flush the rename can land before the data does, which is
        // exactly the torn file we are trying to prevent.
        file.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("meteor-jsonstore-tests");
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp() {
        let path = temp("atomic.json");
        write_atomic(&path, b"[1,2,3]").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1,2,3]");
        write_atomic(&path, b"[4]").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[4]");
        assert!(!path.with_extension("tmp").exists());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn quarantine_moves_the_file_aside_instead_of_deleting_it() {
        // Regression (C5): a corrupt store used to fall back to defaults and get
        // overwritten on the next save, losing the user's data silently.
        let path = temp("corrupt.json");
        fs::write(&path, b"{not json").unwrap();
        quarantine(&path, "corrupt.json", "test");
        assert!(!path.exists(), "the corrupt file must be moved away");
        let dir = path.parent().unwrap();
        let backups: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("corrupt.json.corrupt-")
            })
            .collect();
        assert!(!backups.is_empty(), "a .corrupt-<ts> copy must exist");
        assert_eq!(
            fs::read_to_string(backups[0].path()).unwrap(),
            "{not json",
            "the original bytes must be preserved"
        );
        for b in backups {
            let _ = fs::remove_file(b.path());
        }
    }
}
