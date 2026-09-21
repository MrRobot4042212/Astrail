// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Export and import of the data only the user can recreate.
//!
//! A rescan rebuilds the library, the covers and the icons. It cannot rebuild the
//! manual apps, the play time, the favorites, the categories, the hidden list, the
//! covers the user picked or the settings. Those go into **one JSON file**:
//!
//! ```json
//! { "format": "astrail-backup", "version": 1, "app_version": "0.3.0", "created": 1789000000,
//!   "stores": { "favorites.json": ["steam:440"], "hidden.json": null },
//!   "user_covers": { "0123456789abcdef.png": "<base64>" } }
//! ```
//!
//! A store that does not exist yet is written as `null`, and restoring it removes
//! the file, so a restore gives back the state that was saved rather than a mix. A
//! store that could not be read is left out of the file, and a restore does not
//! touch it. Derived files (`library_cache.json`, `hidden_cache.json`, `covers/`,
//! `app_icons/`), in-flight sessions and logs are never included.
//!
//! **A backup file is untrusted input.** It may come from another machine, from an
//! older or newer Astrail, or from someone else. So every store is deserialized
//! into its real type, manual apps can only be manual apps (a backup cannot plant
//! a protocol URI), cover file names must be the 16 hex digits `art::cache_key`
//! produces plus a known image extension (no path can leave `user_covers/`), sizes
//! are capped, and error messages never quote the file.
//!
//! A restore **replaces**; it does not merge (merging play time would count the
//! same sessions twice). Before anything is replaced, the current data is saved
//! under `backups/` in the data dir.
//!
//! Everything here takes the data dir as a plain path and runs on whatever thread
//! calls it: the commands in `lib.rs` call it from the blocking pool, the tests
//! from a temp dir. The file dialogs and the settings state live in `lib.rs`.

use crate::art;
use crate::jsonstore;
use crate::models::{AppSettings, Game, GameSource};
use crate::playtime::{self, PlayStat};
use crate::storage;
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};

const FORMAT: &str = "astrail-backup";
const VERSION: u32 = 1;

/// Every store a backup carries. Anything else found in a file is ignored.
const STORES: &[&str] = &[
    storage::STORE_FILE,
    storage::OVERRIDES_FILE,
    storage::HIDDEN_FILE,
    storage::FAVORITES_FILE,
    storage::CATEGORIES_FILE,
    storage::CATEGORY_NAMES_FILE,
    storage::CATEGORY_ICONS_FILE,
    storage::TYPE_OVERRIDES_FILE,
    storage::DISCORD_FILE,
    storage::SETTINGS_FILE,
    playtime::STORE_FILE,
];

const COVERS_DIR: &str = "user_covers";
pub(crate) const SAFETY_DIR: &str = "backups";
/// Pre-restore copies kept. Each one may carry every user cover.
const SAFETY_KEEP: usize = 2;
/// Automatic copies (`auto-<ts>.json` in `SAFETY_DIR`): at most one per
/// `AUTO_EVERY_SECS`, newest `AUTO_KEEP` kept. Play time only exists on this
/// machine, and a disk that dies takes it along; the export button only helps
/// people who remember to press it.
const AUTO_KEEP: usize = 4;
const AUTO_EVERY_SECS: u64 = 7 * 24 * 60 * 60;
const AUTO_PREFIX: &str = "auto-";
const SAFETY_PREFIX: &str = "pre-import-";

/// Largest backup file that is read at all. An export that would be larger is
/// refused too, so Astrail never writes a backup it cannot read back.
const MAX_BUNDLE_BYTES: u64 = 160 * 1024 * 1024;
/// Same cap the download of a pasted cover URL has.
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;
/// Raw cover bytes one backup carries. Base64 makes them a third larger (128 MB),
/// which leaves 32 MB of `MAX_BUNDLE_BYTES` for the stores.
const MAX_COVERS_TOTAL: u64 = 96 * 1024 * 1024;
const MAX_ID_LEN: usize = 512;
const MAX_NAME_LEN: usize = 256;
const MAX_VALUE_LEN: usize = 2048;

#[derive(Serialize, Deserialize)]
struct Bundle {
    format: String,
    version: u32,
    #[serde(default)]
    app_version: String,
    #[serde(default)]
    created: u64,
    #[serde(default)]
    stores: BTreeMap<String, Value>,
    #[serde(default)]
    user_covers: BTreeMap<String, String>,
}

/// What an export wrote, for the settings card.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ExportReport {
    pub path: String,
    pub covers: usize,
    /// Covers left out because the file would have grown past its cap.
    pub covers_skipped: usize,
    /// Stores that exist but could not be read; they are not in the file.
    pub unreadable: Vec<String>,
}

/// What a backup file holds, shown before the user confirms the restore.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportSummary {
    pub file_name: String,
    pub created: u64,
    pub app_version: String,
    pub manual_apps: usize,
    pub played_games: usize,
    pub favorites: usize,
    pub hidden: usize,
    pub categories: usize,
    pub covers: usize,
    pub includes_settings: bool,
}

/// What a restore did.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    pub covers: usize,
    /// Where the data that was replaced went.
    pub safety_copy: String,
}

/// One store of a backup: not in the file, saved as "did not exist", or a value.
#[derive(Debug, Default)]
enum Slot<T> {
    #[default]
    Absent,
    Cleared,
    Value(T),
}

impl<T> Slot<T> {
    fn value(&self) -> Option<&T> {
        match self {
            Slot::Value(v) => Some(v),
            _ => None,
        }
    }
}

/// A backup that passed validation, ready to be written.
#[derive(Debug, Default)]
pub struct Restore {
    manual: Slot<Vec<Game>>,
    cover_overrides: Slot<HashMap<String, String>>,
    hidden: Slot<Vec<String>>,
    favorites: Slot<Vec<String>>,
    categories: Slot<HashMap<String, Vec<String>>>,
    category_names: Slot<Vec<String>>,
    category_icons: Slot<HashMap<String, String>>,
    type_overrides: Slot<HashMap<String, String>>,
    playtime: Slot<HashMap<String, PlayStat>>,
    discord_id: Slot<String>,
    /// Applied by the caller, under the settings lock (`lib.rs`).
    pub settings: Option<AppSettings>,
    covers: Vec<(String, Vec<u8>)>,
    pub summary: ImportSummary,
}

impl Restore {
    /// The Rich Presence client id to switch to, when the backup changes it.
    pub fn discord_id(&self) -> Option<&str> {
        match &self.discord_id {
            Slot::Absent => None,
            Slot::Cleared => Some(""),
            Slot::Value(id) => Some(id),
        }
    }
}

// --- Export -----------------------------------------------------------------

/// `<16 lowercase hex>.<image extension>`: the only names `user_covers/` holds,
/// and the only ones a backup may write there.
fn is_cover_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    stem.len() == 16
        && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && storage::COVER_EXTS.contains(&ext)
}

/// The `user_covers/` file a stored cover value points at, if it is one.
fn cover_file_name(value: &str) -> Option<&str> {
    if art::is_remote(value) {
        return None;
    }
    let name = value.rsplit(['\\', '/']).next()?;
    is_cover_name(name).then_some(name)
}

/// Read a store for export. `Ok(Value::Null)` = not there yet; `Err` = there but
/// unreadable. Never quarantines: an export must not move the user's files.
fn read_store(dir: &Path, file: &str) -> Result<Value, ()> {
    let text = match fs::read_to_string(dir.join(file)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(e) => {
            log::warn!("backup: could not read {file}: {e}");
            return Err(());
        }
    };
    // Same rule as `jsonstore::load`: an empty file is a torn first write.
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| {
        log::warn!("backup: {file} is not valid JSON ({e}); left out");
    })
}

/// Cover values the stores point at: the overrides and the manual apps' own.
fn referenced_covers(stores: &BTreeMap<String, Value>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut push = |value: &str| {
        if let Some(name) = cover_file_name(value) {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    };
    if let Some(map) = stores.get(storage::OVERRIDES_FILE).and_then(Value::as_object) {
        map.values().filter_map(Value::as_str).for_each(&mut push);
    }
    if let Some(list) = stores.get(storage::STORE_FILE).and_then(Value::as_array) {
        list.iter()
            .filter_map(|game| game.get("cover_url").and_then(Value::as_str))
            .for_each(&mut push);
    }
    names
}

fn collect(dir: &Path, now: u64) -> (Bundle, ExportReport) {
    let mut report = ExportReport::default();
    let mut stores = BTreeMap::new();
    for file in STORES {
        match read_store(dir, file) {
            Ok(value) => {
                stores.insert((*file).to_string(), value);
            }
            Err(()) => report.unreadable.push((*file).to_string()),
        }
    }

    let mut user_covers = BTreeMap::new();
    let mut total: u64 = 0;
    for name in referenced_covers(&stores) {
        let path = dir.join(COVERS_DIR).join(&name);
        let Ok(meta) = fs::metadata(&path) else {
            continue; // the value outlived its file; nothing to carry
        };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > MAX_COVER_BYTES || total + meta.len() > MAX_COVERS_TOTAL {
            report.covers_skipped += 1;
            continue;
        }
        match fs::read(&path) {
            Ok(bytes) => {
                total += bytes.len() as u64;
                user_covers.insert(name, base64::engine::general_purpose::STANDARD.encode(bytes));
            }
            Err(e) => {
                log::warn!("backup: could not read cover {name}: {e}");
                report.covers_skipped += 1;
            }
        }
    }
    report.covers = user_covers.len();

    let bundle = Bundle {
        format: FORMAT.to_string(),
        version: VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created: now,
        stores,
        user_covers,
    };
    (bundle, report)
}

/// Stream the bundle to a sibling temp file, flush it and rename it into place.
fn write_bundle(path: &Path, bundle: &Bundle) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| "The backup path has no file name".to_string())?
        .to_string_lossy();
    // Not `with_extension("tmp")`: this lands in a folder the user chose, where a
    // `<name>.tmp` may be a file of theirs.
    let tmp = path.with_file_name(format!("{name}.astrail-tmp"));
    let written = (|| -> Result<(), String> {
        let file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut out = BufWriter::new(file);
        serde_json::to_writer(&mut out, bundle).map_err(|e| e.to_string())?;
        let file = out.into_inner().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        // `read` refuses anything larger; a backup that cannot be restored is
        // worse than no backup, because the user believes they have one.
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        if len > MAX_BUNDLE_BYTES {
            return Err("the data is larger than a backup file may be".into());
        }
        Ok(())
    })();
    written
        .and_then(|()| fs::rename(&tmp, path).map_err(|e| e.to_string()))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("Could not write the backup: {e}")
        })
}

/// Write the user's data in `dir` to `target`.
pub fn export(dir: &Path, target: &Path, now: u64) -> Result<ExportReport, String> {
    let (bundle, mut report) = collect(dir, now);
    write_bundle(target, &bundle)?;
    report.path = target.to_string_lossy().into_owned();
    Ok(report)
}

/// Save the current data under `backups/` before a restore replaces it, keeping
/// the newest `SAFETY_KEEP`.
pub fn write_safety_copy(dir: &Path, now: u64) -> Result<PathBuf, String> {
    let folder = dir.join(SAFETY_DIR);
    fs::create_dir_all(&folder).map_err(|e| format!("Could not create {SAFETY_DIR}: {e}"))?;
    let path = folder.join(format!("{SAFETY_PREFIX}{now}.json"));
    let (bundle, _) = collect(dir, now);
    write_bundle(&path, &bundle)?;
    prune_copies(&folder, SAFETY_PREFIX, SAFETY_KEEP);
    Ok(path)
}

/// Whether an automatic copy is due, given the newest one's stamp. A stamp in
/// the future (the clock went back) counts as due, or no copy would be made
/// until the clock caught up.
fn auto_due(newest: Option<u64>, now: u64) -> bool {
    newest.is_none_or(|t| t > now || now - t >= AUTO_EVERY_SECS)
}

/// Write `backups/auto-<now>.json` if the newest automatic copy is older than
/// `AUTO_EVERY_SECS`, keeping the newest `AUTO_KEEP`. `Ok(None)` = not due, or
/// nothing to save yet (no store exists: a fresh install).
pub fn auto_copy_if_due(dir: &Path, now: u64) -> Result<Option<PathBuf>, String> {
    let folder = dir.join(SAFETY_DIR);
    if !auto_due(copies(&folder, AUTO_PREFIX).first().map(|c| c.0), now) {
        return Ok(None);
    }
    if !STORES.iter().any(|file| dir.join(file).is_file()) {
        return Ok(None);
    }
    fs::create_dir_all(&folder).map_err(|e| format!("Could not create {SAFETY_DIR}: {e}"))?;
    let path = folder.join(format!("{AUTO_PREFIX}{now}.json"));
    let (bundle, _) = collect(dir, now);
    write_bundle(&path, &bundle)?;
    prune_copies(&folder, AUTO_PREFIX, AUTO_KEEP);
    Ok(Some(path))
}

/// `auto_copy_if_due` on the app's data dir, logging the outcome. Called at
/// start-up and after each recorded session; when not due it costs one
/// directory listing.
pub fn auto_backup(app: &tauri::AppHandle) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now == 0 {
        return;
    }
    let result = crate::jsonstore::data_dir(app).and_then(|dir| auto_copy_if_due(&dir, now));
    match result {
        Ok(Some(path)) => log::info!("automatic backup written to {}", path.display()),
        Ok(None) => {}
        Err(e) => log::warn!("automatic backup failed: {e}"),
    }
}

fn copy_stamp(name: &str, prefix: &str) -> Option<u64> {
    name.strip_prefix(prefix)?.strip_suffix(".json")?.parse().ok()
}

/// Copies named `<prefix><ts>.json` in `folder`, newest first.
fn copies(folder: &Path, prefix: &str) -> Vec<(u64, PathBuf)> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut copies: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| Some((copy_stamp(&e.file_name().to_string_lossy(), prefix)?, e.path())))
        .collect();
    copies.sort_by(|a, b| b.0.cmp(&a.0));
    copies
}

fn prune_copies(folder: &Path, prefix: &str, keep: usize) {
    for (_, path) in copies(folder, prefix).into_iter().skip(keep) {
        if let Err(e) = fs::remove_file(&path) {
            log::warn!("backup: could not remove old safety copy {}: {e}", path.display());
        }
    }
}

// --- Import: read and validate ------------------------------------------------

/// Hashes everything read through it and refuses to read past `limit`.
struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    seen: u64,
    limit: u64,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.seen += n as u64;
        if self.seen > self.limit {
            return Err(io::Error::other("backup file too large"));
        }
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

/// Read and validate a backup file. Returns what it holds and the SHA-256 of the
/// bytes that were validated, so `read_verified` can refuse a file that changed
/// between the summary the user saw and the restore.
pub fn read(path: &Path) -> Result<(Restore, [u8; 32]), String> {
    let file = fs::File::open(path).map_err(|e| format!("Could not open the backup: {e}"))?;
    let len = file.metadata().map_err(|e| format!("Could not open the backup: {e}"))?.len();
    if len > MAX_BUNDLE_BYTES {
        return Err("The backup file is too large".into());
    }
    let mut reader = BufReader::new(HashingReader {
        inner: file,
        hasher: Sha256::new(),
        seen: 0,
        limit: MAX_BUNDLE_BYTES,
    });
    // Line and column only: a serde message can quote the text it choked on.
    let bundle: Bundle = serde_json::from_reader(&mut reader).map_err(|e| {
        format!("Not a valid Astrail backup (line {}, column {})", e.line(), e.column())
    })?;
    // `from_reader` stops at the end of the value; hash whatever follows too.
    io::copy(&mut reader, &mut io::sink()).map_err(|e| format!("Could not read the backup: {e}"))?;
    let hash: [u8; 32] = reader.into_inner().hasher.finalize().into();

    let mut restore = validate(bundle)?;
    restore.summary.file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((restore, hash))
}

/// `read`, refusing a file whose bytes are not the ones that were checked.
pub fn read_verified(path: &Path, expected: &[u8; 32]) -> Result<Restore, String> {
    let (restore, hash) = read(path)?;
    if &hash != expected {
        return Err("The backup file changed after it was checked; pick it again".into());
    }
    Ok(restore)
}

fn slot<T: DeserializeOwned>(stores: &mut BTreeMap<String, Value>, file: &str) -> Result<Slot<T>, String> {
    match stores.remove(file) {
        None => Ok(Slot::Absent),
        Some(Value::Null) => Ok(Slot::Cleared),
        // The serde message is dropped on purpose: it quotes the offending value.
        Some(value) => serde_json::from_value(value)
            .map(Slot::Value)
            .map_err(|_| format!("{file} in the backup does not have the expected shape")),
    }
}

fn fits(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max
}

fn clean_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| fits(id, MAX_ID_LEN) && seen.insert(id.clone()))
        .collect()
}

fn clean_names(names: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    names
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| fits(n, MAX_NAME_LEN) && seen.insert(n.clone()))
        .collect()
}

/// A manual app is a name and an executable the user pointed at, nothing else. A
/// backup cannot turn one into a store entry or give it a protocol URI to run;
/// the executable is validated at launch like any other (`launcher.rs`).
fn clean_manual(games: Vec<Game>) -> Vec<Game> {
    let mut seen = HashSet::new();
    games
        .into_iter()
        .filter(|g| g.id.starts_with("manual:") && fits(&g.id, MAX_ID_LEN) && fits(&g.name, MAX_NAME_LEN))
        .filter(|g| seen.insert(g.id.clone()))
        .map(|g| Game {
            id: g.id,
            name: g.name.trim().to_string(),
            source: GameSource::Manual,
            app_id: None,
            executable: g.executable.filter(|p| fits(p, MAX_VALUE_LEN)),
            install_dir: g.install_dir.filter(|p| fits(p, MAX_VALUE_LEN)),
            cover_url: g.cover_url.filter(|c| fits(c, MAX_VALUE_LEN)),
            launch_uri: None,
            favorite: false,
            categories: Vec::new(),
        })
        .collect()
}

fn clean_playtime(map: HashMap<String, PlayStat>) -> HashMap<String, PlayStat> {
    map.into_iter()
        .filter(|(id, _)| fits(id, MAX_ID_LEN))
        .map(|(id, mut stat)| {
            stat.history.retain(|s| s.end >= s.start);
            // Same bound the watcher keeps; the dropped sessions stay in `seconds`.
            if stat.history.len() > playtime::HISTORY_MAX {
                let overflow = stat.history.len() - playtime::HISTORY_MAX;
                stat.history.drain(..overflow);
            }
            (id, stat)
        })
        .collect()
}

fn map_slot<T, F: FnOnce(T) -> T>(slot: Slot<T>, clean: F) -> Slot<T> {
    match slot {
        Slot::Value(v) => Slot::Value(clean(v)),
        other => other,
    }
}

fn validate(mut bundle: Bundle) -> Result<Restore, String> {
    if bundle.format != FORMAT {
        return Err("Not an Astrail backup".into());
    }
    if bundle.version == 0 || bundle.version > VERSION {
        return Err("This backup was made by a newer Astrail; update first".into());
    }

    let stores = &mut bundle.stores;
    let manual = map_slot(slot::<Vec<Game>>(stores, storage::STORE_FILE)?, clean_manual);
    let cover_overrides = map_slot(
        slot::<HashMap<String, String>>(stores, storage::OVERRIDES_FILE)?,
        |map| {
            map.into_iter()
                .filter(|(id, value)| fits(id, MAX_ID_LEN) && fits(value, MAX_VALUE_LEN))
                .collect()
        },
    );
    let hidden = map_slot(slot::<Vec<String>>(stores, storage::HIDDEN_FILE)?, clean_ids);
    let favorites = map_slot(slot::<Vec<String>>(stores, storage::FAVORITES_FILE)?, clean_ids);
    let categories = map_slot(
        slot::<HashMap<String, Vec<String>>>(stores, storage::CATEGORIES_FILE)?,
        |map| {
            map.into_iter()
                .filter(|(id, _)| fits(id, MAX_ID_LEN))
                .map(|(id, names)| (id, clean_names(names)))
                .filter(|(_, names)| !names.is_empty())
                .collect()
        },
    );
    let category_names = map_slot(slot::<Vec<String>>(stores, storage::CATEGORY_NAMES_FILE)?, clean_names);
    let category_icons = map_slot(
        slot::<HashMap<String, String>>(stores, storage::CATEGORY_ICONS_FILE)?,
        |map| {
            map.into_iter()
                .filter(|(name, icon)| fits(name, MAX_NAME_LEN) && fits(icon, MAX_NAME_LEN))
                .collect()
        },
    );
    // Same rule as `storage::set_type_override`: "app" or "game", nothing else.
    let type_overrides = map_slot(
        slot::<HashMap<String, String>>(stores, storage::TYPE_OVERRIDES_FILE)?,
        |map| {
            map.into_iter()
                .filter(|(id, kind)| fits(id, MAX_ID_LEN) && matches!(kind.as_str(), "app" | "game"))
                .collect()
        },
    );
    let playtime = map_slot(
        slot::<HashMap<String, PlayStat>>(stores, playtime::STORE_FILE)?,
        clean_playtime,
    );
    // A Discord application id is a number; anything else is not restored.
    let discord_id = match slot::<String>(stores, storage::DISCORD_FILE)? {
        Slot::Value(id) => {
            let id = id.trim().to_string();
            if id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit()) {
                Slot::Value(id)
            } else {
                Slot::Absent
            }
        }
        other => other,
    };
    // A saved "no settings yet" does not reset the settings of a running app.
    let settings = match slot::<AppSettings>(stores, storage::SETTINGS_FILE)? {
        Slot::Value(mut settings) => {
            // The read-time migrations `storage::load_settings` applies.
            settings.shortcuts.migrate_legacy_defaults();
            settings.overlay.migrate_legacy_gpu();
            Some(settings)
        }
        _ => None,
    };
    if !stores.is_empty() {
        log::info!("backup: {} unknown store(s) in the file ignored", stores.len());
    }

    let mut covers = Vec::with_capacity(bundle.user_covers.len());
    let mut total: u64 = 0;
    // By value: each base64 string is freed as soon as it is decoded.
    for (name, encoded) in std::mem::take(&mut bundle.user_covers) {
        if !is_cover_name(&name) {
            return Err("The backup holds a cover with a file name Astrail never writes".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|_| "The backup holds a cover that is not valid base64".to_string())?;
        total += bytes.len() as u64;
        if bytes.is_empty() || bytes.len() as u64 > MAX_COVER_BYTES || total > MAX_COVERS_TOTAL {
            return Err("The backup holds a cover that is empty or too large".into());
        }
        covers.push((name, bytes));
    }

    let summary = ImportSummary {
        file_name: String::new(),
        created: bundle.created,
        app_version: bundle
            .app_version
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
            .take(32)
            .collect(),
        manual_apps: manual.value().map_or(0, Vec::len),
        played_games: playtime.value().map_or(0, |m| m.values().filter(|s| s.seconds > 0).count()),
        favorites: favorites.value().map_or(0, Vec::len),
        hidden: hidden.value().map_or(0, Vec::len),
        categories: category_names.value().map_or(0, Vec::len),
        covers: covers.len(),
        includes_settings: settings.is_some(),
    };

    Ok(Restore {
        manual,
        cover_overrides,
        hidden,
        favorites,
        categories,
        category_names,
        category_icons,
        type_overrides,
        playtime,
        discord_id,
        settings,
        covers,
        summary,
    })
}

// --- Import: write ------------------------------------------------------------

/// A cover value that names a file this backup carries now lives in *this* data
/// dir (the backup may come from another user profile or machine). Any other
/// value is kept as it was.
fn remap_cover(value: &str, dir: &Path, carried: &HashSet<&str>) -> String {
    match cover_file_name(value) {
        Some(name) if carried.contains(name) => {
            dir.join(COVERS_DIR).join(name).to_string_lossy().into_owned()
        }
        _ => value.to_string(),
    }
}

fn write_slot<T: Serialize>(dir: &Path, file: &str, slot: &Slot<T>) -> Result<(), String> {
    let path = dir.join(file);
    match slot {
        Slot::Absent => Ok(()),
        Slot::Cleared => match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Could not remove {file}: {e}")),
        },
        Slot::Value(value) => {
            let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
            jsonstore::write_atomic(&path, text.as_bytes())
                .map_err(|e| format!("Could not write {file}: {e}"))
        }
    }
}

/// Write the covers and every store except the settings (the caller owns the
/// settings state and its lock). Returns the number of covers written.
///
/// Call `write_safety_copy` first. A failure half way leaves some stores restored
/// and some not; the safety copy is what gets the user back.
pub fn apply(dir: &Path, restore: &Restore) -> Result<usize, String> {
    // A store whose corrupt content could not be moved aside must not be replaced
    // either: that content is the only copy of whatever the user had.
    if let Some(file) = STORES.iter().find(|f| jsonstore::is_poisoned(f)) {
        return Err(format!("{file} holds damaged data that could not be set aside; nothing was imported"));
    }

    // Covers first: they are the large part, and no lock is held for them.
    let folder = dir.join(COVERS_DIR);
    if !restore.covers.is_empty() {
        fs::create_dir_all(&folder).map_err(|e| format!("Could not create {COVERS_DIR}: {e}"))?;
    }
    for (name, bytes) in &restore.covers {
        // One cover per id: drop the same id under another extension, as
        // `storage::save_cover_image` does.
        if let (Some((stem, _)), Ok(entries)) = (name.split_once('.'), fs::read_dir(&folder)) {
            for entry in entries.flatten() {
                let other = entry.file_name().to_string_lossy().into_owned();
                if other != *name && is_cover_name(&other) && other.starts_with(stem) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        jsonstore::write_atomic(&folder.join(name), bytes)
            .map_err(|e| format!("Could not write a cover: {e}"))?;
    }

    let carried: HashSet<&str> = restore.covers.iter().map(|(name, _)| name.as_str()).collect();
    let manual = match &restore.manual {
        Slot::Value(games) => Slot::Value(
            games
                .iter()
                .cloned()
                .map(|mut g| {
                    g.cover_url = g.cover_url.map(|c| remap_cover(&c, dir, &carried));
                    g
                })
                .collect::<Vec<Game>>(),
        ),
        Slot::Cleared => Slot::Cleared,
        Slot::Absent => Slot::Absent,
    };
    let cover_overrides = match &restore.cover_overrides {
        Slot::Value(map) => Slot::Value(
            map.iter()
                .map(|(id, value)| (id.clone(), remap_cover(value, dir, &carried)))
                .collect::<HashMap<String, String>>(),
        ),
        Slot::Cleared => Slot::Cleared,
        Slot::Absent => Slot::Absent,
    };

    write_slot(dir, storage::STORE_FILE, &manual)?;
    write_slot(dir, storage::OVERRIDES_FILE, &cover_overrides)?;
    write_slot(dir, storage::HIDDEN_FILE, &restore.hidden)?;
    write_slot(dir, storage::FAVORITES_FILE, &restore.favorites)?;
    write_slot(dir, storage::CATEGORIES_FILE, &restore.categories)?;
    write_slot(dir, storage::CATEGORY_NAMES_FILE, &restore.category_names)?;
    write_slot(dir, storage::CATEGORY_ICONS_FILE, &restore.category_icons)?;
    write_slot(dir, storage::TYPE_OVERRIDES_FILE, &restore.type_overrides)?;
    write_slot(dir, storage::DISCORD_FILE, &restore.discord_id)?;
    {
        // The watcher appends sessions with a read-modify-write of this file.
        let _guard = playtime::store_guard();
        write_slot(dir, playtime::STORE_FILE, &restore.playtime)?;
    }
    Ok(restore.covers.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const COVER: &str = "0123456789abcdef.png";

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("astrail-backup-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(dir: &Path, file: &str, value: &Value) {
        fs::write(dir.join(file), serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn get(dir: &Path, file: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.join(file)).unwrap()).unwrap()
    }

    fn manual(id: &str, cover: Option<String>) -> Value {
        json!({ "id": id, "name": "Emulator", "source": "manual",
                "executable": r"D:\Emu\emu.exe", "cover_url": cover })
    }

    /// A data dir with a bit of everything, and one user cover on disk.
    fn seeded(name: &str) -> PathBuf {
        let dir = temp(name);
        let cover = dir.join(COVERS_DIR).join(COVER);
        fs::create_dir_all(cover.parent().unwrap()).unwrap();
        fs::write(&cover, b"\x89PNG fake").unwrap();
        let cover = cover.to_string_lossy().into_owned();
        put(&dir, storage::STORE_FILE, &json!([manual("manual:1", Some(cover.clone()))]));
        put(&dir, storage::OVERRIDES_FILE, &json!({ "steam:440": cover, "gog:1": "https://example.com/a.jpg" }));
        put(&dir, storage::FAVORITES_FILE, &json!(["steam:440"]));
        put(&dir, storage::CATEGORY_NAMES_FILE, &json!(["RPG"]));
        put(&dir, storage::CATEGORIES_FILE, &json!({ "steam:440": ["RPG"] }));
        put(&dir, storage::TYPE_OVERRIDES_FILE, &json!({ "windows:x": "app" }));
        put(
            &dir,
            playtime::STORE_FILE,
            &json!({ "steam:440": { "seconds": 3600, "last_played": 1_700_000_000u64,
                     "history": [{ "start": 1_699_996_400u64, "end": 1_700_000_000u64 }] } }),
        );
        dir
    }

    fn bundle_with(stores: Value, covers: Value) -> PathBuf {
        let dir = temp(&format!("bundle-{:016x}", {
            use std::hash::{BuildHasher, Hasher};
            std::collections::hash_map::RandomState::new().build_hasher().finish()
        }));
        let path = dir.join("b.json");
        let body = json!({ "format": FORMAT, "version": VERSION, "stores": stores, "user_covers": covers });
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        path
    }

    #[test]
    fn a_backup_restores_into_another_data_dir() {
        let from = seeded("roundtrip-from");
        let to = temp("roundtrip-to");
        let file = to.join("out.json");
        let report = export(&from, &file, 1_789_000_000).unwrap();
        assert_eq!((report.covers, report.covers_skipped), (1, 0));
        assert!(report.unreadable.is_empty());
        assert!(!to.join("out.json.astrail-tmp").exists());

        let (restore, hash) = read(&file).unwrap();
        assert_eq!(restore.summary.manual_apps, 1);
        assert_eq!(restore.summary.played_games, 1);
        assert_eq!(restore.summary.favorites, 1);
        assert_eq!(restore.summary.categories, 1);
        assert_eq!(restore.summary.covers, 1);
        assert_eq!(restore.summary.created, 1_789_000_000);
        assert!(!restore.summary.includes_settings);

        let restore = read_verified(&file, &hash).unwrap();
        assert_eq!(apply(&to, &restore).unwrap(), 1);

        assert_eq!(fs::read(to.join(COVERS_DIR).join(COVER)).unwrap(), b"\x89PNG fake");
        // The cover values follow the file into the new data dir; a URL is kept.
        let here = to.join(COVERS_DIR).join(COVER).to_string_lossy().into_owned();
        let overrides = get(&to, storage::OVERRIDES_FILE);
        assert_eq!(overrides["steam:440"], json!(here));
        assert_eq!(overrides["gog:1"], json!("https://example.com/a.jpg"));
        assert_eq!(get(&to, storage::STORE_FILE)[0]["cover_url"], json!(here));
        assert_eq!(get(&to, storage::FAVORITES_FILE), json!(["steam:440"]));
        assert_eq!(get(&to, playtime::STORE_FILE)["steam:440"]["seconds"], json!(3600));
        assert_eq!(get(&to, storage::TYPE_OVERRIDES_FILE), json!({ "windows:x": "app" }));
    }

    #[test]
    fn a_store_saved_as_missing_is_removed_and_one_not_in_the_file_is_kept() {
        let to = temp("null-and-absent");
        put(&to, storage::HIDDEN_FILE, &json!(["steam:1"]));
        put(&to, storage::FAVORITES_FILE, &json!(["steam:2"]));
        let file = bundle_with(json!({ "hidden.json": null }), json!({}));
        let (restore, _) = read(&file).unwrap();
        apply(&to, &restore).unwrap();
        assert!(!to.join(storage::HIDDEN_FILE).exists());
        assert_eq!(get(&to, storage::FAVORITES_FILE), json!(["steam:2"]));
    }

    #[test]
    fn an_unreadable_store_is_left_out_and_never_moved() {
        let dir = seeded("corrupt");
        fs::write(dir.join(storage::FAVORITES_FILE), b"{ not json").unwrap();
        let file = dir.join("out.json");
        let report = export(&dir, &file, 1).unwrap();
        assert_eq!(report.unreadable, vec![storage::FAVORITES_FILE.to_string()]);
        // Still where it was: an export does not quarantine.
        assert_eq!(fs::read(dir.join(storage::FAVORITES_FILE)).unwrap(), b"{ not json");

        // Restoring that backup leaves the favorites of the target alone.
        let to = temp("corrupt-to");
        put(&to, storage::FAVORITES_FILE, &json!(["gog:9"]));
        let (restore, _) = read(&file).unwrap();
        apply(&to, &restore).unwrap();
        assert_eq!(get(&to, storage::FAVORITES_FILE), json!(["gog:9"]));
    }

    #[test]
    fn a_manual_app_from_a_backup_stays_a_manual_app() {
        let file = bundle_with(
            json!({ "manual_apps.json": [
                { "id": "manual:7", "name": "  Tool  ", "source": "steam", "app_id": 440,
                  "executable": r"C:\t.exe", "launch_uri": "evil://run", "favorite": true,
                  "categories": ["x"] },
                { "id": "steam:440", "name": "Posing", "source": "steam" },
                { "id": "manual:7", "name": "Duplicate", "source": "manual" },
                { "id": "manual:8", "name": "   ", "source": "manual" }
            ] }),
            json!({}),
        );
        let (restore, _) = read(&file).unwrap();
        let games = restore.manual.value().unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!((g.id.as_str(), g.name.as_str()), ("manual:7", "Tool"));
        assert_eq!(g.source, GameSource::Manual);
        assert_eq!((g.app_id, g.launch_uri.as_deref(), g.favorite), (None, None, false));
        assert!(g.categories.is_empty());
    }

    #[test]
    fn a_cover_name_that_could_leave_the_folder_is_refused() {
        for name in [
            "../0123456789abcdef.png",
            r"..\0123456789abcdef.png",
            "0123456789abcdef.exe",
            "0123456789ABCDEF.png",
            "0123456789abcdef.png.exe",
            "0123456789abcde.png",
            "0123456789abcdef",
            r"C:\Windows\0123456789abcdef.png",
        ] {
            assert!(!is_cover_name(name), "{name}");
            let file = bundle_with(json!({}), json!({ name: "AAAA" }));
            assert!(read(&file).is_err(), "{name}");
        }
        assert!(is_cover_name(COVER));
        assert!(read(&bundle_with(json!({}), json!({ COVER: "AAAA" }))).is_ok());
        assert!(read(&bundle_with(json!({}), json!({ COVER: "not base64!" }))).is_err());
        assert!(read(&bundle_with(json!({}), json!({ COVER: "" }))).is_err());
    }

    #[test]
    fn a_cover_value_is_only_remapped_to_a_file_the_backup_carries() {
        let dir = Path::new(r"C:\data");
        let carried: HashSet<&str> = [COVER].into_iter().collect();
        let here = dir.join(COVERS_DIR).join(COVER).to_string_lossy().into_owned();
        assert_eq!(remap_cover(&format!(r"D:\old\user_covers\{COVER}"), dir, &carried), here);
        assert_eq!(remap_cover(&format!("/old/user_covers/{COVER}"), dir, &carried), here);
        let other = r"D:\old\user_covers\fedcba9876543210.jpg";
        assert_eq!(remap_cover(other, dir, &carried), other);
        let url = format!("https://example.com/{COVER}");
        assert_eq!(remap_cover(&url, dir, &carried), url);
        assert_eq!(remap_cover(r"D:\Fotoñ\a.png", dir, &carried), r"D:\Fotoñ\a.png");
    }

    #[test]
    fn another_format_or_a_newer_version_is_refused() {
        let dir = temp("format");
        let path = dir.join("b.json");
        for body in [
            json!({ "format": "something-else", "version": 1 }),
            json!({ "format": FORMAT, "version": VERSION + 1 }),
            json!({ "format": FORMAT, "version": 0 }),
            json!({ "version": 1 }),
            json!([1, 2, 3]),
        ] {
            fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
            assert!(read(&path).is_err(), "{body}");
        }
        fs::write(&path, b"{ truncated").unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn a_file_that_changed_after_the_check_is_refused() {
        let file = bundle_with(json!({ "favorites.json": ["steam:1"] }), json!({}));
        let (_, hash) = read(&file).unwrap();
        assert!(read_verified(&file, &hash).is_ok());
        // Trailing bytes count too: the hash covers the whole file.
        let mut bytes = fs::read(&file).unwrap();
        bytes.extend_from_slice(b"\n ");
        fs::write(&file, &bytes).unwrap();
        assert!(read_verified(&file, &hash).is_err());
        let swapped = bundle_with(json!({ "favorites.json": ["steam:2"] }), json!({}));
        assert!(read_verified(&swapped, &hash).is_err());
    }

    #[test]
    fn what_a_backup_may_hold_is_bounded() {
        let sessions: Vec<Value> = (0..playtime::HISTORY_MAX as u64 + 20)
            .map(|i| json!({ "start": i * 100, "end": i * 100 + 50 }))
            .chain([json!({ "start": 900, "end": 100 })])
            .collect();
        let file = bundle_with(
            json!({
                "type_overrides.json": { "a": "app", "b": "game", "c": "rootkit", "": "app" },
                "favorites.json": ["x", "x", "", "  "],
                "categories.json": { "x": ["  RPG ", "", "RPG"], "y": [] },
                "discord.json": "not a number",
                "playtime.json": { "x": { "seconds": 99, "last_played": null, "history": sessions } },
                "someday.json": { "ignored": true }
            }),
            json!({}),
        );
        let (restore, _) = read(&file).unwrap();
        let kinds = restore.type_overrides.value().unwrap();
        assert_eq!(kinds.len(), 2);
        assert_eq!(restore.favorites.value().unwrap(), &vec!["x".to_string()]);
        let categories = restore.categories.value().unwrap();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories["x"], vec!["RPG".to_string()]);
        assert!(restore.discord_id().is_none());
        let stat = &restore.playtime.value().unwrap()["x"];
        assert_eq!(stat.history.len(), playtime::HISTORY_MAX);
        assert_eq!(stat.seconds, 99);
        // The newest sessions are the ones kept.
        assert_eq!(stat.history.last().unwrap().start, (playtime::HISTORY_MAX as u64 + 19) * 100);
    }

    #[test]
    fn an_error_never_quotes_the_file() {
        let file = bundle_with(json!({ "favorites.json": "hunter2-secret" }), json!({}));
        let err = read(&file).unwrap_err();
        assert!(err.contains("favorites.json"), "{err}");
        assert!(!err.contains("hunter2"), "{err}");

        let dir = temp("quote");
        let path = dir.join("b.json");
        fs::write(&path, br#"{ "format": "astrail-backup", "version": "hunter2-secret" }"#).unwrap();
        assert!(!read(&path).unwrap_err().contains("hunter2"));
    }

    #[test]
    fn settings_in_a_backup_are_typed_and_migrated() {
        let file = bundle_with(
            json!({ "app_settings.json": { "language": "en", "shortcuts": { "spotlight": "F9" } } }),
            json!({}),
        );
        let (restore, _) = read(&file).unwrap();
        assert!(restore.summary.includes_settings);
        let settings = restore.settings.unwrap();
        assert_eq!(settings.language, "en");
        // A bare F-key never comes back as a machine-wide hotkey.
        assert_ne!(settings.shortcuts.spotlight, "F9");

        let wrong = bundle_with(json!({ "app_settings.json": { "language": 5 } }), json!({}));
        assert!(read(&wrong).is_err());
        let none = bundle_with(json!({ "app_settings.json": null }), json!({}));
        assert!(read(&none).unwrap().0.settings.is_none());
    }

    #[test]
    fn only_the_newest_safety_copies_are_kept() {
        let dir = seeded("safety");
        for now in [10, 30, 20, 40] {
            let path = write_safety_copy(&dir, now).unwrap();
            assert!(read(&path).is_ok());
        }
        let mut left: Vec<String> = fs::read_dir(dir.join(SAFETY_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["pre-import-30.json", "pre-import-40.json"]);
    }

    #[test]
    fn automatic_copies_are_weekly_bounded_and_readable() {
        let dir = seeded("auto");
        let week = AUTO_EVERY_SECS;
        let first = auto_copy_if_due(&dir, 1_000).unwrap().expect("first copy");
        assert!(read(&first).is_ok());
        // Not due again within the week, due right after it.
        assert!(auto_copy_if_due(&dir, 1_000 + week - 1).unwrap().is_none());
        for i in 1..=5 {
            assert!(auto_copy_if_due(&dir, 1_000 + i * week).unwrap().is_some());
        }
        let left = copies(&dir.join(SAFETY_DIR), AUTO_PREFIX);
        assert_eq!(left.len(), AUTO_KEEP);
        assert_eq!(left[0].0, 1_000 + 5 * week);
        // Safety copies of a restore are a separate series, never pruned by these.
        write_safety_copy(&dir, 5).unwrap();
        auto_copy_if_due(&dir, 1_000 + 6 * week).unwrap();
        assert_eq!(copies(&dir.join(SAFETY_DIR), SAFETY_PREFIX).len(), 1);
    }

    #[test]
    fn a_fresh_install_writes_no_automatic_copy() {
        let dir = temp("auto-empty");
        assert!(auto_copy_if_due(&dir, 1_000).unwrap().is_none());
        assert!(!dir.join(SAFETY_DIR).exists());
    }

    #[test]
    fn a_clock_that_went_back_does_not_stop_automatic_copies() {
        assert!(auto_due(None, 10));
        assert!(!auto_due(Some(10), 10 + AUTO_EVERY_SECS - 1));
        assert!(auto_due(Some(10), 10 + AUTO_EVERY_SECS));
        assert!(auto_due(Some(500), 10));
    }
}
