//! Cover-art pipeline: resolve a game name to a cover image, cached on disk.
//!
//! Layers, fastest first:
//! 1. The image file is already downloaded → return its path. No lock, no
//!    network, one `stat`.
//! 2. The IGDB image id is cached → download the image (no search API call).
//! 3. Ask IGDB once, remember the id (or the miss), then download.
//!
//! Two things this module is careful about, because the frontend resolves
//! covers for a whole library in parallel:
//! - the URL cache lives behind an `RwLock` and is **never cloned** per lookup;
//! - it is written back at most once every `SAVE_INTERVAL` instead of once per
//!   resolved cover (which used to rewrite the whole file N times per scan).

use crate::igdb;
use crate::jsonstore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

/// Process-wide HTTP agent for cover downloads. Reuses TCP/TLS connections
/// across all parallel downloads, cutting per-request handshake overhead.
static DOWNLOAD_AGENT: OnceLock<ureq::Agent> = OnceLock::new();

fn agent() -> &'static ureq::Agent {
    DOWNLOAD_AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(6))
            .timeout_read(Duration::from_secs(15))
            .build()
    })
}

const CACHE_FILE: &str = "cover_cache.json";
const COVERS_DIR: &str = "covers";
/// Current on-disk schema. v1 was a bare `{name: {url, ts}}` map; v2 stores the
/// IGDB **image id** instead of a full URL, so the served variant can change
/// without re-resolving every game.
const CACHE_VERSION: u32 = 2;
/// A "no cover found" result is only trusted for a while, then re-tried — so a
/// transient network blip (or IGDB creds added later) self-heals.
const NEGATIVE_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);
/// Write the URL cache back to disk at most this often during a cover pass.
const SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// Cap for the downloaded-covers directory, pruned least-recently-used first.
const COVERS_MAX_BYTES: u64 = 200 * 1024 * 1024;

/// One URL-cache entry.
#[derive(Clone, Serialize, Deserialize, Default)]
struct Entry {
    /// IGDB image id (e.g. `co1r7h`). Empty = looked up, nothing found.
    #[serde(default)]
    image_id: String,
    /// v1 field: the full image URL. Kept so an old cache still resolves; it is
    /// back-filled into `image_id` on load and then dropped.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    url: String,
    ts: u64,
}

impl Entry {
    /// True when this entry records a successful lookup.
    fn is_hit(&self) -> bool {
        !self.image_id.is_empty() || !self.url.is_empty()
    }
}

#[derive(Serialize, Deserialize, Default)]
struct CacheFile {
    version: u32,
    entries: HashMap<String, Entry>,
}

/// The URL cache, loaded once. `RwLock` because every parallel `resolve` reads
/// it and only a miss writes.
static CACHE: OnceLock<RwLock<HashMap<String, Entry>>> = OnceLock::new();
/// Last flush time plus whether there are unsaved changes.
static SAVE_STATE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Stable filename key (FNV-1a).
///
/// `DefaultHasher` is documented as unstable across Rust releases, so a
/// toolchain bump silently invalidated every cached cover: they were
/// re-downloaded under new names and the old files leaked forever. See
/// `migrate_filenames`.
pub fn cache_key(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The pre-FNV filename key (`DefaultHasher`), for the one-time rename.
pub(crate) fn legacy_key(key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// IGDB image variants. The grid renders ~240×360 CSS px, so `t_cover_big`
/// (264×374) is the right size there; `t_cover_big_2x` (528×748) is four times
/// the decoded bytes and only earns that on the detail hero.
const VARIANT_GRID: &str = "t_cover_big";
const VARIANT_HIRES: &str = "t_cover_big_2x";

fn image_url(image_id: &str, variant: &str) -> String {
    format!("https://images.igdb.com/igdb/image/upload/{variant}/{image_id}.jpg")
}

/// An IGDB image id is an opaque slug (`co1r76`), never a path or a URL.
///
/// Guards the one place a value enters the cache's `image_id` field: a full URL
/// stored there composes into `…/upload/t_cover_big/https://…jpg.jpg`, which is
/// how every freshly resolved IGDB cover ended up with a broken link.
fn is_valid_image_id(s: &str) -> bool {
    !s.is_empty() && !s.contains('/') && !s.contains(':') && !s.contains('.')
}

/// Extract the image id out of a v1 cached URL.
fn image_id_from_url(url: &str) -> String {
    url.rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".jpg"))
        .unwrap_or_default()
        .to_string()
}

/// Resolve the **grid** cover for a game name (downloading it if needed).
pub fn resolve(app: &AppHandle, name: &str) -> Option<String> {
    let _span = crate::perf::Span::new("art::resolve");
    resolve_variant(app, name, VARIANT_GRID)
}

/// Resolve the **high-resolution** cover for the detail page hero. Reuses the
/// cached image id, so it costs a download at most — never a search.
pub fn resolve_hires(app: &AppHandle, name: &str) -> Option<String> {
    resolve_variant(app, name, VARIANT_HIRES)
}

/// The cached cover path for a name, or `None` — **never touches the network**.
///
/// `get_library` uses this to fill `cover_url` for entries whose image is
/// already on disk, so a refresh no longer asks the frontend to re-resolve every
/// non-Steam game over IPC.
pub fn cached_path(app: &AppHandle, name: &str) -> Option<String> {
    let key = name.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let file = cover_file(app, &key, VARIANT_GRID).ok()?;
    file.exists().then(|| file.to_string_lossy().to_string())
}

fn resolve_variant(app: &AppHandle, name: &str, variant: &str) -> Option<String> {
    let key = name.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }

    // 1. Already downloaded? Serve the local file — no lock, no network.
    let file = cover_file(app, &key, variant).ok()?;
    if file.exists() {
        return Some(file.to_string_lossy().to_string());
    }

    // 2. Known image id (cached) avoids re-hitting the IGDB search API.
    let cached = {
        let cache = cache(app).read().ok()?;
        cache.get(&key).cloned()
    };
    let image_id = match cached {
        Some(entry) if entry.is_hit() => {
            if entry.image_id.is_empty() {
                image_id_from_url(&entry.url)
            } else {
                entry.image_id
            }
        }
        // A recent miss: don't ask again until the negative TTL expires.
        Some(entry) if !is_stale(entry.ts) => return None,
        _ => {
            // 3. Ask IGDB once and remember the result — but only when IGDB
            //    actually answered. A network failure used to be stored as an empty
            //    entry and honoured as "this game has no cover" for the whole
            //    negative TTL, so one offline scan blanked the library for days.
            let variants = name_variants(name);
            let resolved = match igdb::resolve_cover(&variants) {
                igdb::Lookup::Found(id) if is_valid_image_id(&id) => id,
                igdb::Lookup::Found(bad) => {
                    // Never cache something that is not an id: it would compose
                    // into a malformed image URL and stay wrong until the cache is
                    // wiped by hand.
                    eprintln!("[art] ignoring malformed IGDB image id: {bad}");
                    return None;
                }
                // Could not ask: leave the cache untouched so the next attempt
                // retries instead of inheriting a fabricated miss.
                igdb::Lookup::Unavailable => return None,
                igdb::Lookup::NotFound => String::new(),
            };
            if let Ok(mut cache) = cache(app).write() {
                cache.insert(
                    key.clone(),
                    Entry {
                        image_id: resolved.clone(),
                        url: String::new(),
                        ts: now(),
                    },
                );
            }
            save_cache(app, false);
            if resolved.is_empty() {
                return None;
            }
            resolved
        }
    };

    let url = image_url(&image_id, variant);
    // Download to disk; serve the local file, or the remote URL if it failed.
    if download(&url, &file) {
        Some(file.to_string_lossy().to_string())
    } else {
        Some(url)
    }
}

/// Drop the URL cache and every downloaded image (e.g. after the IGDB
/// credentials change) so covers are resolved and re-fetched from scratch.
pub fn clear_cache(app: &AppHandle) -> Result<(), String> {
    if let Ok(mut cache) = cache(app).write() {
        cache.clear();
    }
    jsonstore::remove(app, CACHE_FILE)?;
    if let Ok(dir) = covers_dir(app) {
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Progressively looser search terms: the exact name, then without trademark
/// symbols, then without edition/qualifier suffixes. Deduplicated, order kept.
fn name_variants(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if !s.is_empty() && !out.iter().any(|e| e.eq_ignore_ascii_case(&s)) {
            out.push(s);
        }
    };

    push(name.to_string());

    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '™' | '®' | '©' | '℠'))
        .collect();
    push(cleaned.clone());

    // Strip common edition/qualifier suffixes for a last, looser attempt.
    const SUFFIXES: &[&str] = &[
        "game of the year edition",
        "goty edition",
        "definitive edition",
        "complete edition",
        "ultimate edition",
        "deluxe edition",
        "gold edition",
        "remastered",
        "directors cut",
        "director's cut",
    ];
    let lower = cleaned.to_lowercase();
    for suffix in SUFFIXES {
        if let Some(pos) = lower.rfind(suffix) {
            if pos + suffix.len() >= lower.len().saturating_sub(1) {
                push(
                    cleaned[..pos]
                        .trim_end_matches([' ', '-', ':', '–'])
                        .to_string(),
                );
                break;
            }
        }
    }

    out
}

/// Download an image to `dest`. Returns true on success.
fn download(url: &str, dest: &Path) -> bool {
    let Ok(resp) = agent().get(url).call() else {
        return false;
    };
    let mut bytes = Vec::new();
    if resp.into_reader().read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return false;
    }
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    // Atomic: a half-written .jpg would be cached forever as a broken image.
    jsonstore::write_atomic(dest, &bytes).is_ok()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_stale(ts: u64) -> bool {
    now().saturating_sub(ts) > NEGATIVE_TTL.as_secs()
}

fn covers_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(jsonstore::data_dir(app)?.join(COVERS_DIR))
}

/// Deterministic on-disk path for a cached cover image. The hi-res variant gets
/// its own `@2x` file so both can coexist.
fn cover_file(app: &AppHandle, key: &str, variant: &str) -> Result<PathBuf, String> {
    let suffix = if variant == VARIANT_HIRES { "@2x" } else { "" };
    Ok(covers_dir(app)?.join(format!("{}{suffix}.jpg", cache_key(key))))
}

/// The URL cache, read from disk on first use.
fn cache(app: &AppHandle) -> &'static RwLock<HashMap<String, Entry>> {
    CACHE.get_or_init(|| RwLock::new(load_cache(app)))
}

fn load_cache(app: &AppHandle) -> HashMap<String, Entry> {
    let raw: serde_json::Value = jsonstore::load_or_default(app, CACHE_FILE);
    // v2 is `{version, entries}`; v1 was the bare map. Tell them apart by shape,
    // because a bare map would deserialize into a v2 struct with zero entries.
    let mut entries: HashMap<String, Entry> = if raw.get("entries").is_some() {
        serde_json::from_value::<CacheFile>(raw)
            .map(|c| c.entries)
            .unwrap_or_default()
    } else {
        serde_json::from_value(raw).unwrap_or_default()
    };
    // Back-fill v1 entries: derive the image id from the stored URL once.
    for entry in entries.values_mut() {
        if entry.image_id.is_empty() && !entry.url.is_empty() {
            entry.image_id = image_id_from_url(&entry.url);
            entry.url = String::new();
        }
    }
    entries
}

/// Persist the URL cache, at most once every `SAVE_INTERVAL` unless `force`.
///
/// A cover pass resolves hundreds of names; writing the whole map after each one
/// turned a single scan into hundreds of full-file rewrites.
fn save_cache(app: &AppHandle, force: bool) {
    let should_write = {
        let mut state = SAVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        match *state {
            Some((last, _)) if !force && last.elapsed() < SAVE_INTERVAL => {
                *state = Some((last, true)); // dirty; a later call flushes it
                false
            }
            _ => {
                *state = Some((Instant::now(), false));
                true
            }
        }
    };
    if !should_write {
        return;
    }
    let Some(lock) = CACHE.get() else { return };
    let Ok(entries) = lock.read() else { return };
    let file = CacheFile {
        version: CACHE_VERSION,
        entries: entries.clone(),
    };
    if let Err(e) = jsonstore::save(app, CACHE_FILE, &file) {
        eprintln!("[art] could not save {CACHE_FILE}: {e}");
    }
}

/// Flush pending cache changes (called when a cover pass ends).
pub fn flush(app: &AppHandle) {
    let dirty = SAVE_STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map(|(_, dirty)| dirty)
        .unwrap_or(false);
    if dirty {
        save_cache(app, true);
    }
}

/// One-time migration of cache filenames from `DefaultHasher` to FNV-1a, plus
/// the user covers that used the same scheme. A miss costs a re-download, never
/// data: user covers are renamed through the paths in `cover_overrides.json`.
pub fn migrate_filenames(app: &AppHandle) {
    if let (Ok(dir), Ok(entries)) = (covers_dir(app), cache(app).read()) {
        if dir.exists() {
            for key in entries.keys() {
                let old = dir.join(format!("{}.jpg", legacy_key(key)));
                let new = dir.join(format!("{}.jpg", cache_key(key)));
                if old.exists() && !new.exists() {
                    let _ = fs::rename(&old, &new);
                }
            }
        }
    }
    crate::storage::migrate_user_cover_filenames(app);
}

/// Keep `covers/` under `COVERS_MAX_BYTES`, dropping the least recently used
/// files first. Downloaded art is fully re-creatable, so pruning is safe; before
/// this the directory grew without any bound at all.
pub fn prune_covers(app: &AppHandle) {
    let Ok(dir) = covers_dir(app) else { return };
    let freed = prune_lru(&dir, COVERS_MAX_BYTES);
    if freed > 0 {
        eprintln!(
            "[art] pruned {} MB of cached covers (cap {} MB)",
            freed / (1024 * 1024),
            COVERS_MAX_BYTES / (1024 * 1024)
        );
    }
}

/// Keep the files directly under `dir` at or below `cap` bytes, deleting the
/// least recently used first. Returns the bytes freed. Shared by every
/// re-creatable cache directory (`covers/`, `app_icons/`).
pub(crate) fn prune_lru(dir: &Path, cap: u64) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if !md.is_file() {
                return None;
            }
            let used = md.accessed().or_else(|_| md.modified()).ok()?;
            Some((e.path(), md.len(), used))
        })
        .collect();
    let total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= cap {
        return 0;
    }
    files.sort_by_key(|(_, _, used)| *used); // oldest first
    let mut freed = 0u64;
    for (path, len, _) in files {
        if total - freed <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            freed += len;
        }
    }
    freed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_url_is_never_accepted_as_an_image_id() {
        // Regression: `igdb::resolve_cover` used to return a composed
        // `t_cover_big_2x` URL, which was stored in the cache's `image_id` field
        // and fed back into `image_url`, producing a URL nested inside a URL.
        assert!(!is_valid_image_id(
            "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co1r76.jpg"
        ));
        assert!(!is_valid_image_id("t_cover_big/co1r76"));
        assert!(!is_valid_image_id("co1r76.jpg"));
        assert!(!is_valid_image_id(""));
        assert!(is_valid_image_id("co1r76"));
    }

    #[test]
    fn a_valid_image_id_composes_a_url_it_can_be_read_back_from() {
        let url = image_url("co1r76", VARIANT_GRID);
        assert_eq!(url, format!("https://images.igdb.com/igdb/image/upload/{VARIANT_GRID}/co1r76.jpg"));
        assert_eq!(image_id_from_url(&url), "co1r76");
    }

    #[test]
    fn cache_key_is_stable_across_builds() {
        // The whole point of dropping DefaultHasher: these must never change.
        assert_eq!(cache_key(""), "cbf29ce484222325");
        assert_eq!(cache_key("a"), "af63dc4c8601ec8c");
        assert_eq!(cache_key("halo infinite"), cache_key("halo infinite"));
        assert_ne!(cache_key("halo"), cache_key("halo 2"));
    }

    #[test]
    fn image_id_round_trips_through_a_v1_url() {
        let url = image_url("co1r7h", VARIANT_HIRES);
        assert_eq!(
            url,
            "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co1r7h.jpg"
        );
        assert_eq!(image_id_from_url(&url), "co1r7h");
        assert_eq!(image_id_from_url("nonsense"), "");
    }

    #[test]
    fn name_variants_loosen_progressively() {
        let v = name_variants("Halo™: The Master Chief Collection");
        assert_eq!(v[0], "Halo™: The Master Chief Collection");
        assert!(v.contains(&"Halo: The Master Chief Collection".to_string()));

        let v = name_variants("Skyrim Definitive Edition");
        assert!(
            v.iter().any(|s| s == "Skyrim"),
            "the edition suffix should produce a looser variant: {v:?}"
        );

        // Deduplicated: a plain ASCII name yields exactly one variant.
        assert_eq!(name_variants("Portal 2").len(), 1);
    }

    #[test]
    fn grid_and_hires_use_different_files() {
        assert_ne!(
            format!("{}.jpg", cache_key("halo")),
            format!("{}@2x.jpg", cache_key("halo"))
        );
    }

    #[test]
    fn prune_lru_drops_the_least_recently_used_files_down_to_the_cap() {
        use std::fs::{File, FileTimes};
        use std::time::Duration;
        let dir = std::env::temp_dir().join("meteor-prune-lru-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let base = SystemTime::now() - Duration::from_secs(3600);
        // Three 100-byte files, oldest first; "new" is the most recently used.
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            let path = dir.join(format!("{name}.ico"));
            fs::write(&path, [0u8; 100]).unwrap();
            let t = base + Duration::from_secs(60 * i as u64);
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(FileTimes::new().set_accessed(t).set_modified(t))
                .unwrap();
        }
        assert_eq!(prune_lru(&dir, 300), 0, "under the cap nothing is touched");
        assert_eq!(prune_lru(&dir, 150), 200);
        assert!(!dir.join("old.ico").exists());
        assert!(!dir.join("mid.ico").exists());
        assert!(dir.join("new.ico").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
