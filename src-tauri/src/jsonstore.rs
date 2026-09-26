// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Atomic JSON persistence for every store in the app data dir.
//!
//! Astrail keeps one JSON file per concern (manual apps, favorites, categories,
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
//!
//! **Schema version.** `schema.json` records the layout version of the whole data
//! dir (`SCHEMA_VERSION`). It is a separate file on purpose: wrapping every store
//! in `{"v":N,"data":…}` would make any older build — including an installed copy
//! running next to a newer one on the same data dir — read each store as corrupt
//! and quarantine it. `check_schema` runs once at start-up: a missing file means
//! the layout every build before it wrote (version 1), an older version runs the
//! migrations in order, and a *newer* version makes this build refuse every store
//! write for the rest of the session, so it cannot overwrite data it does not
//! understand.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
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

pub(crate) fn is_poisoned(file: &str) -> bool {
    poisoned()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(file)
}

/// Layout version of the data dir that this build reads and writes. Bump it
/// together with a step in `migrate` whenever a store changes shape in a way an
/// older build would misread.
pub const SCHEMA_VERSION: u32 = 1;
const SCHEMA_FILE: &str = "schema.json";

/// The newer schema version found at start-up, or 0. Non-zero = read-only data.
static NEWER_SCHEMA: AtomicU32 = AtomicU32::new(0);

/// What `check_schema` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    /// The recorded version is this build's.
    Current,
    /// An older layout (or none recorded) was brought up to this build's.
    Upgraded { from: u32 },
    /// A newer build wrote this data: every store write is refused.
    Newer { found: u32 },
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SchemaFile {
    version: u32,
}

/// Decide what to do with the recorded version (`None` = no `schema.json`,
/// i.e. the layout every build before this file existed wrote: version 1).
fn schema_for(recorded: Option<u32>) -> Schema {
    let found = recorded.unwrap_or(1);
    match found.cmp(&SCHEMA_VERSION) {
        std::cmp::Ordering::Equal if recorded.is_some() => Schema::Current,
        std::cmp::Ordering::Greater => Schema::Newer { found },
        _ => Schema::Upgraded { from: found },
    }
}

/// Bring the data dir from `from` to `SCHEMA_VERSION`, one step per version.
/// Version 1 is the first recorded layout, so there is nothing to run yet.
fn migrate(_dir: &Path, from: u32) -> Result<(), String> {
    for version in from..SCHEMA_VERSION {
        #[allow(clippy::match_single_binding)]
        match version {
            // A step `n => migrate_n_to_n_plus_1(dir)?,` goes here.
            _ => {}
        }
    }
    Ok(())
}

/// Read `schema.json`, migrate or lock the data dir, and record the version.
/// Call once at start-up, before any store is written.
pub fn check_schema(app: &AppHandle) -> Schema {
    let Ok(dir) = data_dir(app) else { return Schema::Current };
    let state = check_schema_in(&dir);
    if let Schema::Newer { found } = state {
        NEWER_SCHEMA.store(found, Ordering::Relaxed);
    }
    state
}

fn check_schema_in(dir: &Path) -> Schema {
    let path = dir.join(SCHEMA_FILE);
    let recorded = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<SchemaFile>(&text).ok())
        .map(|f| f.version);
    let state = schema_for(recorded);
    match state {
        Schema::Current => {}
        Schema::Newer { found } => {
            log::warn!(
                "the data dir has schema {found}, newer than this build's {SCHEMA_VERSION}:                  this copy will not write any store (update Astrail to change settings again)"
            );
        }
        Schema::Upgraded { from } => {
            if let Err(e) = migrate(dir, from) {
                log::error!("data migration from schema {from} failed: {e}; stores left as they were");
                return state;
            }
            let file = SchemaFile { version: SCHEMA_VERSION };
            match serde_json::to_vec_pretty(&file) {
                Ok(bytes) => match write_atomic(&path, &bytes) {
                    Ok(()) => log::info!("{}", schema_note(from)),
                    Err(e) => log::warn!("could not record the data schema: {e}"),
                },
                Err(e) => log::warn!("could not record the data schema: {e}"),
            }
        }
    }
    state
}

/// Log line for a recorded schema: the first record of a dir that had no
/// `schema.json` is not a migration, so it must not read like one ("1 -> 1").
fn schema_note(from: u32) -> String {
    if from == SCHEMA_VERSION {
        format!("data dir schema {SCHEMA_VERSION} recorded (no schema.json before)")
    } else {
        format!("data dir schema {from} -> {SCHEMA_VERSION}")
    }
}

/// Refuse a store write when a newer build owns the data dir.
pub(crate) fn writable(file: &str) -> Result<(), String> {
    match NEWER_SCHEMA.load(Ordering::Relaxed) {
        0 => Ok(()),
        found => Err(format!(
            "{file} was not saved: the data was written by a newer Astrail (schema {found}, this build reads {SCHEMA_VERSION})"
        )),
    }
}

/// The app data dir, created once per process instead of on every access.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Could not resolve the app data dir: {e}"))?;
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
        Ok(()) => log::error!(
            "{file} is corrupt ({err}); moved to {} and starting from defaults",
            backup.display()
        ),
        Err(e) => {
            log::error!(
                "{file} is corrupt ({err}) and could not be quarantined ({e}); refusing to overwrite it"
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
    writable(file)?;
    if is_poisoned(file) {
        return Err(format!("{file} holds corrupt data that could not be set aside; not overwriting it"));
    }
    let path = path(app, file)?;
    let data = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    write_atomic(&path, data.as_bytes())
}

/// Like `save`, but skips the write when the file already has these exact bytes.
/// Returns whether anything was written.
///
/// This is what keeps idle Astrail off the disk: the playtime watcher rewrote
/// `active_sessions.json` every 5 seconds with the same `[]`.
pub fn save_if_changed<T: Serialize>(
    app: &AppHandle,
    file: &str,
    value: &T,
) -> Result<bool, String> {
    writable(file)?;
    if is_poisoned(file) {
        return Err(format!("{file} holds corrupt data that could not be set aside; not overwriting it"));
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
    writable(file)?;
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

    #[test]
    fn the_schema_version_is_read_migrated_and_recorded() {
        assert_eq!(schema_for(None), Schema::Upgraded { from: 1 });
        assert_eq!(schema_for(Some(SCHEMA_VERSION)), Schema::Current);
        assert_eq!(schema_for(Some(SCHEMA_VERSION + 1)), Schema::Newer { found: SCHEMA_VERSION + 1 });

        let dir = std::env::temp_dir().join(format!("astrail-schema-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // Data from before the file existed: recorded, nothing else touched.
        fs::write(dir.join("favorites.json"), "[\"steam:1\"]").unwrap();
        assert_eq!(check_schema_in(&dir), Schema::Upgraded { from: 1 });
        assert_eq!(check_schema_in(&dir), Schema::Current);
        assert_eq!(fs::read_to_string(dir.join("favorites.json")).unwrap(), "[\"steam:1\"]");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_the_first_schema_is_not_logged_as_a_migration() {
        // Seen in the log of the first run: "data dir schema 1 -> 1".
        assert_eq!(schema_note(SCHEMA_VERSION), format!("data dir schema {SCHEMA_VERSION} recorded (no schema.json before)"));
        if SCHEMA_VERSION > 1 {
            assert_eq!(schema_note(1), format!("data dir schema 1 -> {SCHEMA_VERSION}"));
        }
    }

    #[test]
    fn data_from_a_newer_build_is_never_rewritten() {
        // An older copy (a second install, a downgrade) must not save stores whose
        // layout it does not know; that is how data a newer build wrote gets lost.
        let dir = std::env::temp_dir().join(format!("astrail-schema-newer-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let newer = format!("{{\"version\":{}}}", SCHEMA_VERSION + 1);
        fs::write(dir.join(SCHEMA_FILE), &newer).unwrap();
        assert_eq!(check_schema_in(&dir), Schema::Newer { found: SCHEMA_VERSION + 1 });
        // The newer file is left as it was.
        assert_eq!(fs::read_to_string(dir.join(SCHEMA_FILE)).unwrap(), newer);
        let _ = fs::remove_dir_all(&dir);
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("astrail-jsonstore-tests");
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
