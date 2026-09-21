// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::jsonstore;
use crate::sessionperf::SessionPerf;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

/// Games the watcher may match, as `(id, registered at)`: the ones launched from
/// Astrail, plus, while external tracking is on, the ones whose window came to the
/// foreground on their own (`register_external`). Everything else in the library is
/// never looked for.
static LAUNCHED_FROM_ASTRAIL: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// Whether games started outside Astrail are picked up (setting
/// `track_external_games`, see `fgwatch.rs`).
static EXTERNAL_TRACKING: AtomicBool = AtomicBool::new(false);
/// Set by the foreground hook, consumed by the watcher.
static FOREGROUND_DIRTY: AtomicBool = AtomicBool::new(false);

/// Wake signal for the watcher thread: a generation counter plus a condvar.
///
/// The watcher used to `sleep(5s)` forever and enumerate **every process on the
/// system** on each tick, even with nothing to track — and then discard all of
/// it, because matching is opt-in per launch (ADR-5). Now it blocks here with no
/// timeout while idle and is woken by `notify_launched`, so an idle Astrail does
/// no process work at all.
static WAKE: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

pub(crate) const STORE_FILE: &str = "playtime.json";
/// Serializes every read-modify-write of `playtime.json`. The watcher appends a
/// session by loading the whole map, pushing and saving; a restore from a backup
/// replaces the file. Without this a session ending during a restore would write
/// the pre-restore map back over the restored one.
static STORE_LOCK: Mutex<()> = Mutex::new(());
/// In-flight sessions, persisted so an Astrail crash/close doesn't lose time.
const ACTIVE_FILE: &str = "active_sessions.json";
/// Snapshot of the library the watcher matches processes against.
const LIBRARY_CACHE: &str = "library_cache.json";
/// Poll interval for the global process watcher.
const POLL_SECS: u64 = 5;
/// While a game is confirmed running, do the expensive full process enumeration only
/// this often; cheap per-PID liveness checks (`proc_alive`) cover the polls in between.
const FULL_SCAN_SECS: u64 = 20;
/// Cap on stored sessions per game. `playtime.json` is rewritten whole on every
/// session end, so an unbounded history makes that write grow forever; the
/// overflow is folded into `seconds`, which is what the UI actually shows.
pub(crate) const HISTORY_MAX: usize = 500;
/// Sessions shorter than this are ignored (a crash, a wrong-process match…).
const MIN_SESSION_SECS: u64 = 30;
/// While the tracked set is unchanged, `active_sessions.json` is rewritten at most
/// this often. `last_seen` moves on every poll, so `save_if_changed` never matched
/// and every 5 s tick did a read + tmp write + fsync + rename on the game drive.
/// A crash loses at most this much of the in-flight session.
const ACTIVE_PERSIST_SECS: u64 = 30;

/// Substrings of the path *relative to the install dir* that are never the game
/// itself (crash handlers, redistributables, anti-cheat services…). Matching the
/// relative path rather than the bare file name is what lets the directory name
/// identify a service whose own name does not (`BattlEye\BEService.exe`).
const EXCLUDE: &[&str] = &[
    "crashhandler",
    "crashpad",
    "crashreport",
    "unitycrashhandler",
    "vcredist",
    "vc_redist",
    "redist",
    "dxsetup",
    "directx",
    "dotnet",
    "setup",
    "installer",
    "uninstall",
    "anticheat",
    "battleye",
    "beservice",
];

/// One finished play session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub start: u64,
    pub end: u64,
    /// What the HUD measured during the session, if it measured anything (see
    /// `sessionperf`). Absent in sessions recorded before this existed, in sessions
    /// played with the overlay off, and in sessions recovered after a crash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perf: Option<SessionPerf>,
}

/// Accumulated play stats for one game, keyed by `Game.id`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayStat {
    /// Total seconds played (cached sum of `history`).
    pub seconds: u64,
    /// Unix timestamp of the last session end, if ever played.
    pub last_played: Option<u64>,
    /// Full per-session history (newest appended last).
    #[serde(default)]
    pub history: Vec<Session>,
}

/// An in-flight session being tracked right now, flushed to disk for recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveSession {
    id: String,
    start: u64,
    last_seen: u64,
}

/// Minimal view of a library entry, read from the on-disk library cache.
#[derive(Debug, Clone, Deserialize)]
struct IndexEntry {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    install_dir: Option<String>,
    #[serde(default)]
    executable: Option<String>,
    source: crate::models::GameSource,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load(app: &AppHandle) -> HashMap<String, PlayStat> {
    jsonstore::load_or_default(app, STORE_FILE)
}

/// `kept id -> ids folded into it` from the last library scan
/// (`library::merge_duplicates`). Sessions are stored under the id that was shown
/// when they were played, so a game played as `epic:x` and later shadowed by a
/// `steam:x` copy would lose its time from view; reads fold the aliases back in.
/// The file itself is never rewritten for this: which copy wins can change on the
/// next scan (a store uninstalled), and a read-side view follows it for free.
static ALIASES: RwLock<Option<HashMap<String, Vec<String>>>> = RwLock::new(None);

/// Record the alias map of a finished scan. Returns whether it changed, so the
/// caller can tell the webview to re-read play stats.
pub fn set_aliases(aliases: &HashMap<String, String>) -> bool {
    let mut by_kept: HashMap<String, Vec<String>> = HashMap::new();
    for (dropped, kept) in aliases {
        by_kept.entry(kept.clone()).or_default().push(dropped.clone());
    }
    for dropped in by_kept.values_mut() {
        dropped.sort();
    }
    let mut slot = ALIASES.write().unwrap_or_else(PoisonError::into_inner);
    // No scan yet reads exactly like an empty map, so a library without
    // duplicates never triggers a re-read.
    if slot.as_ref().map_or(by_kept.is_empty(), |cur| cur == &by_kept) {
        *slot = Some(by_kept);
        return false;
    }
    *slot = Some(by_kept);
    true
}

/// One entry's stats folded from its own record and its aliases' records:
/// seconds summed, latest `last_played`, histories interleaved by start and
/// capped to the newest `HISTORY_MAX` (the dropped ones stay counted in
/// `seconds`, as in `push_session`).
fn merge_stats(parts: &[&PlayStat]) -> PlayStat {
    let mut out = PlayStat::default();
    for part in parts {
        out.seconds += part.seconds;
        out.last_played = out.last_played.max(part.last_played);
        out.history.extend(part.history.iter().cloned());
    }
    out.history.sort_by_key(|s| (s.start, s.end));
    if out.history.len() > HISTORY_MAX {
        let overflow = out.history.len() - HISTORY_MAX;
        out.history.drain(..overflow);
    }
    out
}

/// `map` with every kept id's aliases folded in. Records under the dropped ids
/// stay as they are (nothing shows those ids).
fn with_aliases(
    mut map: HashMap<String, PlayStat>,
    aliases: &HashMap<String, Vec<String>>,
) -> HashMap<String, PlayStat> {
    for (kept, dropped) in aliases {
        let parts: Vec<&PlayStat> = std::iter::once(kept)
            .chain(dropped)
            .filter_map(|id| map.get(id))
            .collect();
        if parts.len() > 1 || (parts.len() == 1 && !map.contains_key(kept)) {
            let merged = merge_stats(&parts);
            map.insert(kept.clone(), merged);
        }
    }
    map
}

/// `playtime.json` as the library shows it (aliases folded in).
fn load_view(app: &AppHandle) -> HashMap<String, PlayStat> {
    let map = load(app);
    let aliases = ALIASES.read().unwrap_or_else(PoisonError::into_inner);
    match aliases.as_ref() {
        Some(a) if !a.is_empty() => with_aliases(map, a),
        _ => map,
    }
}

/// Play stats for a single game id (zeroed if never played).
pub fn get(app: &AppHandle, id: &str) -> PlayStat {
    load_view(app).remove(id).unwrap_or_default()
}

/// Play stats for every game that has any (for sorting the whole library).
pub fn all(app: &AppHandle) -> HashMap<String, PlayStat> {
    load_view(app)
}

/// Add one finished session to a game's stats.
fn push_session(stat: &mut PlayStat, start: u64, end: u64, perf: Option<SessionPerf>) {
    stat.seconds += end.saturating_sub(start);
    stat.last_played = Some(end);
    stat.history.push(Session { start, end, perf });
    // Keep the newest `HISTORY_MAX`; the dropped ones stay counted in `seconds`.
    if stat.history.len() > HISTORY_MAX {
        let overflow = stat.history.len() - HISTORY_MAX;
        stat.history.drain(..overflow);
    }
}

/// Hold this across anything that rewrites `playtime.json` (see `STORE_LOCK`).
pub(crate) fn store_guard() -> std::sync::MutexGuard<'static, ()> {
    STORE_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Persist one finished session for a game.
fn record_session(
    app: &AppHandle,
    id: &str,
    start: u64,
    end: u64,
    perf: Option<SessionPerf>,
) -> Result<(), String> {
    let _guard = store_guard();
    let mut map = load(app);
    push_session(map.entry(id.to_string()).or_default(), start, end, perf);
    jsonstore::save(app, STORE_FILE, &map)
}

// --- Active session persistence (crash recovery) ---------------------------

fn active_load(app: &AppHandle) -> Vec<ActiveSession> {
    jsonstore::load_or_default(app, ACTIVE_FILE)
}

/// Flush in-flight sessions, but only when they actually changed.
///
/// This ran unconditionally on every 5 s poll, so an idle Astrail wrote `[]` to
/// disk ~17 000 times a day — the only continuous disk activity at rest.
fn active_save(app: &AppHandle, sessions: &[ActiveSession]) {
    if let Err(e) = jsonstore::save_if_changed(app, ACTIVE_FILE, &sessions) {
        log::warn!("could not save {ACTIVE_FILE}: {e}");
    }
}

/// On startup, close any sessions left dangling by a previous crash/force-quit:
/// record them up to their last confirmed-alive timestamp, then clear the file.
/// Call once from the Tauri `setup` hook, before `start`.
pub fn reconcile(app: &AppHandle) {
    let leftovers = active_load(app);
    if leftovers.is_empty() {
        return;
    }
    active_save(app, &[]);
    for s in &leftovers {
        if s.last_seen.saturating_sub(s.start) >= MIN_SESSION_SECS {
            // What the HUD measured lived in memory and went with the crash.
            let _ = record_session(app, &s.id, s.start, s.last_seen, None);
        }
    }
    let _ = app.emit("playtime-updated", "");
}

/// Registra que un juego fue lanzado a través de Astrail, para que sus métricas
/// sean mostradas en el overlay.
pub fn notify_launched(id: &str) {
    // Never skip the registration on a poisoned mutex: dropping it here would mean
    // the game the user just launched is silently not tracked at all.
    let mut list = LAUNCHED_FROM_ASTRAIL
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    list.retain(|(i, _)| i != id);
    list.push((id.to_string(), now()));
    drop(list);
    wake();
}

/// Turn the detection of games started outside Astrail on or off. Sessions already
/// in progress are not affected.
pub fn set_external_tracking(on: bool) {
    EXTERNAL_TRACKING.store(on, Ordering::Relaxed);
    crate::fgwatch::set_enabled(on);
}

/// The foreground window now belongs to another process (called from the hook
/// thread in `fgwatch.rs`). Only a flag and a wake: the watcher does the matching.
pub fn foreground_changed() {
    FOREGROUND_DIRTY.store(true, Ordering::Relaxed);
    wake();
}

/// Register a game that was started outside Astrail, exactly as a launch from here
/// would have been. Returns `false` when it was registered already.
fn register_external(id: &str) -> bool {
    let mut list = LAUNCHED_FROM_ASTRAIL
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if list.iter().any(|(i, _)| i == id) {
        return false;
    }
    list.push((id.to_string(), now()));
    true
}

/// Wake the watcher thread (a launch happened, or the library changed).
pub fn wake() {
    let (lock, cv) = &WAKE;
    let mut gen = lock.lock().unwrap_or_else(PoisonError::into_inner);
    *gen = gen.wrapping_add(1);
    cv.notify_all();
}

/// Block until `wake()` is called, or until `timeout` elapses when given.
/// `seen` carries the last observed generation so a wake that arrives between
/// two waits is never missed.
fn wait_for_work(seen: &mut u64, timeout: Option<Duration>) {
    let (lock, cv) = &WAKE;
    let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let guard = match timeout {
        Some(d) => cv
            .wait_timeout_while(guard, d, |gen| *gen == *seen)
            .map(|(g, _)| g)
            .unwrap_or_else(|e| e.into_inner().0),
        None => cv
            .wait_while(guard, |gen| *gen == *seen)
            .unwrap_or_else(PoisonError::into_inner),
    };
    *seen = *guard;
}

// --- Global process watcher ------------------------------------------------

/// Library entries to watch, read from the on-disk cache written by
/// `get_library`. Empty until the first scan completes.
fn library_index(app: &AppHandle) -> Vec<IndexEntry> {
    let entries: Vec<IndexEntry> = jsonstore::load_or_default(app, LIBRARY_CACHE);
    // Keep only entries we can actually match a process against.
    entries
        .into_iter()
        .filter(|e| {
            e.install_dir.as_deref().is_some_and(|s| !s.trim().is_empty())
                || e.executable.as_deref().is_some_and(|s| !s.trim().is_empty())
        })
        .collect()
}

/// Last-modified time of the library cache, so the index is re-read when
/// `get_library` rewrites it instead of on a fixed 60 s timer.
fn library_cache_mtime(app: &AppHandle) -> Option<SystemTime> {
    let path = jsonstore::path(app, LIBRARY_CACHE).ok()?;
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Every running process as `(pid, lowercased exe path)`.
///
/// Straight Win32 (toolhelp snapshot + `QueryFullProcessImageNameW`) instead of
/// `sysinfo`: this was the last real user of that crate, and its Windows
/// process-time code has an unguarded subtraction that panics in debug builds —
/// which killed this very thread. Paths we cannot read (protected/system
/// processes) are skipped, exactly as before.
#[cfg(windows)]
fn running_processes() -> Vec<(u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed on all paths; the entry struct carries
    // its own dwSize as the API requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                if let Some(path) = process_path(pid) {
                    out.push((pid, path));
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// Lowercased executable path of one process, or `None` when it cannot be read
/// (pid 0, protected/system processes, already exited).
#[cfg(windows)]
fn process_path(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed before returning on every path; `buf` outlives
    // the call and `len` carries its capacity in, the written length out.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).to_lowercase())
    }
}

#[cfg(not(windows))]
fn running_processes() -> Vec<(u32, String)> {
    Vec::new()
}

/// Path of `path` relative to `dir`, or `None` when `path` is not inside `dir`.
///
/// The comparison lands on a path-separator boundary. A bare `starts_with` also
/// accepts a sibling that merely shares a textual prefix, so an install dir of
/// `C:\Games\Foo` used to claim every process under `C:\Games\FooBar`, and the
/// playtime clock, the HUD and PresentMon all attached to the wrong game.
/// Both arguments are expected to be lowercased already.
fn relative_to_dir<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    let dir = dir.trim_end_matches(['\\', '/']);
    if dir.is_empty() {
        return None;
    }
    path.strip_prefix(dir)?.strip_prefix(['\\', '/'])
}

/// Whether `path` is a candidate game process under `dir` (both lowercased).
///
/// The whole relative path is tested, not just the file name: anti-cheat services
/// and redistributables live in their own subdirectory (`BattlEye\BEService.exe`,
/// `_CommonRedist\vc_redist.x64.exe`), and their file name on its own carries no
/// hint of what they are.
fn under_install_dir(path: &str, dir: &str) -> bool {
    relative_to_dir(path, dir).is_some_and(|rest| !EXCLUDE.iter().any(|x| rest.contains(x)))
}

/// PID of a running process belonging to this entry (for matching + the metrics
/// overlay / PresentMon). `procs` is the `(pid, lowercased exe path)` list captured
/// once per full scan; a `Some` result doubles as "this entry is running".
/// `foreground` is the pid owning the foreground window (0 = unknown).
///
/// The known executable is matched across the whole list first. With both checks in
/// a single pass, process enumeration order decided the winner: a loose install-dir
/// hit early in the list beat the exact executable further down it.
///
/// Within each tier the foreground process wins over snapshot order. Without that,
/// a launcher or helper started before the game (`launcher.exe` next to `game.exe`)
/// became the tracked pid: PresentMon attached to a process that never presents,
/// and the overlay's foreground gate hid the whole HUD.
fn find_pid(
    procs: &[(u32, String)],
    install_dir: Option<&str>,
    exe: Option<&str>,
    foreground: u32,
) -> Option<u32> {
    let pick = |matches: &dyn Fn(&str) -> bool| -> Option<u32> {
        let mut first = None;
        for (pid, path) in procs {
            if matches(path) {
                if foreground != 0 && *pid == foreground {
                    return Some(*pid);
                }
                first.get_or_insert(*pid);
            }
        }
        first
    };

    if let Some(exe) = exe.map(|s| s.to_lowercase()).filter(|e| !e.is_empty()) {
        if let Some(pid) = pick(&|path| path == exe) {
            return Some(pid);
        }
    }

    let dir = install_dir.map(|s| s.to_lowercase())?;
    pick(&|path| under_install_dir(path, &dir))
}

/// Whether an install dir is specific enough to claim a process nobody launched from
/// here. A drive root (`D:\`) would claim every executable on the drive.
fn is_specific_dir(dir: &str) -> bool {
    dir.split(['\\', '/']).filter(|c| !c.is_empty()).count() >= 2
}

/// Library entry that owns the foreground process (`path`, lowercased), for games
/// started outside Astrail. Mirrors `find_pid`'s tiers so that whatever is matched
/// here is found again by the scan: the exact executable first, then the install
/// dir. When install dirs nest, the deepest one wins; on a tie, library order does.
///
/// Applications take part on purpose: a tool installed inside a game's folder owns
/// its own process, and the game must not claim it. Whether the owner may start a
/// session by itself is `tracked_when_started_outside`'s call, not this one's.
fn match_foreground<'a>(index: &'a [IndexEntry], path: &str) -> Option<&'a IndexEntry> {
    let exact = index.iter().find(|e| {
        e.executable
            .as_deref()
            .is_some_and(|x| !x.is_empty() && x.to_lowercase() == path)
    });
    if exact.is_some() {
        return exact;
    }

    let mut best: Option<(usize, &IndexEntry)> = None;
    for e in index {
        let Some(dir) = e.install_dir.as_deref().map(str::to_lowercase) else { continue };
        if !is_specific_dir(&dir) || !under_install_dir(path, &dir) {
            continue;
        }
        let depth = dir.trim_end_matches(['\\', '/']).len();
        if best.is_none_or(|(d, _)| depth > d) {
            best = Some((depth, e));
        }
    }
    best.map(|(_, e)| e)
}

/// What counts as the user's word that a `Windows` entry is a game: the explicit
/// "game" type override, or having played it from Astrail before (`kind` and `stat`
/// are the entry's own records).
fn is_confirmed_game(kind: Option<&str>, stat: Option<&PlayStat>) -> bool {
    kind == Some("game") || stat.is_some_and(|s| s.last_played.is_some())
}

/// Whether an entry may start a play session just by coming to the foreground.
///
/// - Applications never: bringing a browser to the front is not a play session.
/// - `Windows` entries are "probably a game": the registry scan puts everything its
///   application list does not know there. A guess is good enough to show a tile,
///   not to time a session, draw the HUD and publish a Discord status over what may
///   be a tool. They only qualify with the user's word for it (`confirmed_as_game`,
///   see `is_confirmed_game`; lazy because answering it reads two files).
/// - Store games and manually added entries always.
///
/// Every one of them is still timed when launched from Astrail.
fn tracked_when_started_outside(
    source: &crate::models::GameSource,
    confirmed_as_game: impl FnOnce() -> bool,
) -> bool {
    use crate::models::GameSource::{App, Windows};
    match source {
        App => false,
        Windows => confirmed_as_game(),
        _ => true,
    }
}

/// Whether the foreground process (`fg_path`) should replace the tracked pid
/// (`tracked_path`, `None` when unreadable) between full scans. Mirrors `find_pid`'s
/// tiers: the exact executable is never displaced by a mere install-dir hit.
fn prefer_foreground(
    tracked_path: Option<&str>,
    fg_path: &str,
    install_dir: Option<&str>,
    exe: Option<&str>,
) -> bool {
    let exe = exe.map(|s| s.to_lowercase()).filter(|e| !e.is_empty());
    if exe.as_deref() == Some(fg_path) {
        return true;
    }
    if exe.is_some() && exe.as_deref() == tracked_path {
        return false;
    }
    install_dir
        .map(|d| d.to_lowercase())
        .is_some_and(|dir| under_install_dir(fg_path, &dir))
}

/// Whether the in-flight sessions must be written now: the tracked set changed
/// (a session started or ended — never lose that), or the periodic interval
/// elapsed so a crash loses at most `ACTIVE_PERSIST_SECS` of play.
fn should_persist_active(
    persisted: &[(String, u64)],
    current: &[(String, u64)],
    last_persist: Option<u64>,
    ts: u64,
) -> bool {
    persisted != current
        || last_persist.is_none_or(|t| ts.saturating_sub(t) >= ACTIVE_PERSIST_SECS)
}

/// Whether a process with this PID is still alive, via a single cheap Win32 query, so
/// a confirmed-running game can be re-checked between full scans without walking every
/// process on the system.
#[cfg(windows)]
fn proc_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // GetExitCodeProcess reports 259 (STILL_ACTIVE) while the process runs. A process
    // that genuinely exits with 259 is a rare collision the periodic full scan corrects.
    const STILL_ACTIVE: u32 = 259;
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            // Can't open → treat as gone; if it was actually a live, protected process
            // the next full scan re-adds it by path.
            return false;
        };
        if handle.is_invalid() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code).is_ok();
        let _ = CloseHandle(handle);
        // On a query failure, err towards "alive" so we never drop a running session.
        !ok || code == STILL_ACTIVE
    }
}

/// Re-read the index only when `get_library` actually rewrote the cache.
fn refresh_index(app: &AppHandle, index: &mut Vec<IndexEntry>, index_mtime: &mut Option<SystemTime>) {
    let mtime = library_cache_mtime(app);
    if mtime != *index_mtime {
        *index_mtime = mtime;
        *index = library_index(app);
    }
}

/// Start the playtime watcher: a background thread that times the games it was
/// told about, and nothing else. A game gets there by being launched from Astrail
/// (`notify_launched`) or, while external tracking is on, by coming to the
/// foreground on its own (`foreground_changed`). It parks with no timeout while
/// there is nothing to track; it never enumerates processes on a timer for nobody.
/// Sessions are accumulated per game id and the frontend is notified on end.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // id -> (start, last_seen, pid) for sessions currently in progress.
        let mut active: HashMap<String, (u64, u64, u32)> = HashMap::new();
        let mut index = library_index(&app);
        let mut index_mtime = library_cache_mtime(&app);
        // When the last full process enumeration ran. While a game is confirmed
        // running we only do cheap per-PID liveness checks between full scans, so the
        // watcher costs O(active games) instead of O(all processes) during play.
        // A timestamp, not a tick count: a wake is not always a 5 s poll any more
        // (every foreground change is one), and counting wakes would turn a few
        // alt-tabs into a full scan.
        let mut last_full = 0u64;
        // Game id currently shown in Discord Rich Presence (None = nothing).
        let mut presence: Option<String> = None;
        // Debug: last game name published to the overlay, to log only on change.
        let mut dbg_overlay_game: Option<String> = None;
        // Last observed wake generation (see `WAKE`).
        let mut seen: u64 = 0;
        // What `active_sessions.json` holds: sorted (id, start) + when it was written.
        let mut persisted_keys: Vec<(String, u64)> = Vec::new();
        let mut last_persist: Option<u64> = None;

        loop {
            // Nothing tracked and nothing launched → block until something
            // happens. This is the whole idle-cost story: no timer, no wakeups.
            let idle = active.is_empty()
                && LAUNCHED_FROM_ASTRAIL
                    .lock()
                    .map(|l| l.is_empty())
                    .unwrap_or(true);
            wait_for_work(
                &mut seen,
                if idle {
                    None
                } else {
                    Some(Duration::from_secs(POLL_SECS))
                },
            );

            let ts = now();

            // The foreground moved to another process while external tracking is on:
            // is it a library game nobody launched from here? Registering it is all
            // that happens in this block; the scan below picks it up like a launch.
            #[cfg(windows)]
            if FOREGROUND_DIRTY.swap(false, Ordering::Relaxed) && EXTERNAL_TRACKING.load(Ordering::Relaxed) {
                let fg = crate::overlay::foreground_pid();
                if fg != 0 && fg != std::process::id() && !active.values().any(|v| v.2 == fg) {
                    refresh_index(&app, &mut index, &mut index_mtime);
                    let hit = process_path(fg).and_then(|path| {
                        let owner = match_foreground(&index, &path)?;
                        tracked_when_started_outside(&owner.source, || {
                            is_confirmed_game(
                                crate::storage::load_type_overrides(&app).get(&owner.id).map(String::as_str),
                                load_view(&app).get(&owner.id),
                            )
                        })
                        .then(|| owner.id.clone())
                    });
                    if let Some(id) = hit {
                        if !active.contains_key(&id) && register_external(&id) {
                            log::info!("started outside Astrail: {id} (pid {fg})");
                        }
                    }
                }
            }

            // Mantenemos en la lista de "lanzados" a los juegos que sigan en progreso
            // o que hayan sido lanzados hace menos de 2 minutos (por si tardan en abrir).
            // Prune under the lock, then take a copy and release it immediately.
            //
            // `launch_game` runs on the main tao thread and blocks on this same mutex
            // through `notify_launched`. Holding it for the rest of the tick meant a
            // click on Play could wait behind a full process enumeration, a
            // read-modify-write of playtime.json with its fsync, and a Discord IPC
            // call — freezing IPC, the tray and the global shortcuts with it.
            let launched: Vec<(String, u64)> = {
                let mut launched_list = LAUNCHED_FROM_ASTRAIL
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                launched_list.retain(|(id, launch_ts)| {
                    active.contains_key(id) || ts.saturating_sub(*launch_ts) < 120
                });
                launched_list.clone()
            };

            // An Astrail-launched game we haven't matched to a process yet → keep scanning
            // promptly until it shows up (don't wait for the slow full-scan cadence).
            let pending_launch = launched.iter().any(|(id, _)| !active.contains_key(id));

            // Woken with nothing to do (e.g. the launch window expired): go back
            // to sleep instead of enumerating processes for nobody.
            if active.is_empty() && launched.is_empty() {
                continue;
            }

            refresh_index(&app, &mut index, &mut index_mtime);

            // Full enumeration vs. cheap liveness. A full scan is needed while a
            // launch is still pending (we have no PID yet) and periodically to
            // notice a game that restarted itself; otherwise one syscall per
            // tracked PID is enough. Off-Windows there is no cheap liveness
            // primitive, so always scan.
            #[cfg(windows)]
            let mut do_full = pending_launch || ts.saturating_sub(last_full) >= FULL_SCAN_SECS;
            #[cfg(not(windows))]
            let do_full = true;

            let foreground = crate::overlay::foreground_pid();
            let mut running: HashSet<String> = HashSet::new();

            #[cfg(windows)]
            if !do_full {
                // Cheap path: confirm each tracked game's PID is still alive (1 syscall
                // each) instead of enumerating every process on the system.
                let mut fg_path: Option<Option<String>> = None;
                for (id, v) in active.iter_mut() {
                    if !proc_alive(v.2) {
                        // The tracked pid can be a launcher that exits once the game is
                        // up. Closing the session here ended playtime for a game that is
                        // still running, so re-resolve with a full scan instead.
                        do_full = true;
                        break;
                    }
                    v.1 = ts;
                    running.insert(id.clone());
                    if foreground == 0 || foreground == v.2 {
                        continue;
                    }
                    let Some(fg) = fg_path.get_or_insert_with(|| process_path(foreground)).as_deref() else {
                        continue;
                    };
                    let Some(e) = index.iter().find(|e| &e.id == id) else { continue };
                    let tracked = process_path(v.2);
                    if prefer_foreground(tracked.as_deref(), fg, e.install_dir.as_deref(), e.executable.as_deref()) {
                        v.2 = foreground;
                    }
                }
                if do_full {
                    running.clear();
                }
            }

            if do_full {
                last_full = ts;
                // (pid, lowercased exe path) captured once, reused for matching + pid.
                let procs = running_processes();

                for e in &index {
                    // OPT-IN: only games that are active or were registered (launched
                    // from Astrail, or seen in the foreground with external tracking on).
                    let is_active = active.contains_key(&e.id);
                    let was_launched = launched.iter().any(|(l_id, _)| l_id == &e.id);
                    if !is_active && !was_launched {
                        continue;
                    }
                    if let Some(pid) = find_pid(
                        &procs,
                        e.install_dir.as_deref(),
                        e.executable.as_deref(),
                        foreground,
                    ) {
                        running.insert(e.id.clone());
                        if !active.contains_key(&e.id) {
                            log::info!("session started: {} (pid {pid})", e.id);
                        }
                        active
                            .entry(e.id.clone())
                            .and_modify(|v| {
                                v.1 = ts;
                                v.2 = pid;
                            })
                            .or_insert((ts, ts, pid));
                    }
                }
            }

            // Close sessions whose game is no longer running.
            let ended: Vec<String> = active
                .keys()
                .filter(|id| !running.contains(*id))
                .cloned()
                .collect();
            for id in ended {
                if let Some((start, last, _pid)) = active.remove(&id) {
                    let secs = last.saturating_sub(start);
                    // Taken either way, so a session too short to record does not
                    // leave its measurements behind for the next one.
                    let perf = crate::sessionperf::take(&id);
                    if secs >= MIN_SESSION_SECS {
                        log::info!("session ended: {id} ({secs} s, measured: {perf:?})");
                        if let Err(e) = record_session(&app, &id, start, last, perf) {
                            log::error!("could not record the session of {id}: {e}");
                        }
                        let _ = app.emit("playtime-updated", &id);
                    } else {
                        log::info!("session ended: {id} ({secs} s, under {MIN_SESSION_SECS} s: not recorded)");
                    }
                }
            }

            // Discord Rich Presence: show the most recently started running game.
            let primary = running
                .iter()
                .filter_map(|id| active.get(id).map(|(s, _, _)| (id.clone(), *s)))
                .filter(|(id, _)| {
                    index
                        .iter()
                        .find(|e| &e.id == id)
                        .map(|e| e.source != crate::models::GameSource::App)
                        .unwrap_or(true)
                })
                .max_by_key(|(_, s)| *s)
                .map(|(id, _)| id);

            // Publish the foreground game (name + pid) to the metrics overlay ONLY
            // if it was registered (see `LAUNCHED_FROM_ASTRAIL`).
            let show_metrics_for = primary
                .as_ref()
                .filter(|id| launched.iter().any(|(l_id, _)| l_id == *id));
            let game_name = show_metrics_for
                .and_then(|id| index.iter().find(|e| e.id == **id))
                .map(|e| e.name.clone());
            // PID comes from the active map (resolved at scan time) — no extra walk.
            let game_pid = show_metrics_for.and_then(|id| active.get(id).map(|(_, _, pid)| *pid));
            // Debug: surface why the overlay is/ isn't fed a game (transition-only).
            if game_name != dbg_overlay_game {
                log::info!(
                    "watcher: running_primary={:?} registered={} -> hud_target={:?} pid={:?}",
                    primary,
                    show_metrics_for.is_some(),
                    game_name,
                    game_pid
                );
                dbg_overlay_game = game_name.clone();
            }
            // The session summary follows the HUD: same game, same pid. Before the
            // sampler hears about the game, so its first reading has a session.
            crate::sessionperf::set_target(show_metrics_for.map(String::as_str), game_pid);
            crate::metrics::set_current_game(game_name, game_pid);

            if !crate::discord::enabled() {
                // Opt-in only. Forgetting what we published means turning it back
                // on republishes on the next tick instead of waiting for the next
                // game change.
                if presence.is_some() {
                    crate::discord::clear();
                    presence = None;
                }
            } else if primary != presence {
                match &primary {
                    Some(id) => {
                        let name = index
                            .iter()
                            .find(|e| &e.id == id)
                            .map(|e| e.name.clone())
                            .unwrap_or_default();
                        let start = active.get(id).map(|(s, _, _)| *s).unwrap_or(ts);
                        // Only commit `presence` once Discord actually accepted it,
                        // so we keep retrying if Discord isn't up yet (with backoff,
                        // see discord.rs).
                        if crate::discord::set_playing(&name, start) {
                            presence = primary.clone();
                        }
                    }
                    None => {
                        crate::discord::clear();
                        presence = None;
                    }
                }
            }

            // Flush in-progress sessions for crash recovery.
            let mut keys: Vec<(String, u64)> =
                active.iter().map(|(id, (start, _, _))| (id.clone(), *start)).collect();
            keys.sort();
            if should_persist_active(&persisted_keys, &keys, last_persist, ts) {
                let snapshot: Vec<ActiveSession> = active
                    .iter()
                    .map(|(id, (start, last, _pid))| ActiveSession {
                        id: id.clone(),
                        start: *start,
                        last_seen: *last,
                    })
                    .collect();
                active_save(&app, &snapshot);
                persisted_keys = keys;
                last_persist = Some(ts);
            }
        }
    });
}

#[cfg(test)]
mod alias_tests {
    use super::*;

    fn stat(sessions: &[(u64, u64)]) -> PlayStat {
        let mut s = PlayStat::default();
        for &(a, b) in sessions {
            push_session(&mut s, a, b, None);
        }
        s
    }

    #[test]
    fn time_played_under_a_folded_duplicate_is_shown_on_the_kept_entry() {
        // Regression (H4): Hades played as `epic:hades`, then a Steam copy wins
        // the name. The library shows `steam:1`, and its time used to read 0.
        let map = HashMap::from([
            ("epic:hades".to_string(), stat(&[(100, 400)])),
            ("steam:1".to_string(), stat(&[(1_000, 1_060)])),
            ("steam:2".to_string(), stat(&[(5, 10)])),
        ]);
        let aliases = HashMap::from([("steam:1".to_string(), vec!["epic:hades".to_string()])]);
        let view = with_aliases(map, &aliases);
        let hades = &view["steam:1"];
        assert_eq!(hades.seconds, 360);
        assert_eq!(hades.last_played, Some(1_060));
        assert_eq!(hades.history.iter().map(|s| s.start).collect::<Vec<_>>(), vec![100, 1_000]);
        // Unrelated entries are untouched, and the dropped record is not deleted.
        assert_eq!(view["steam:2"].seconds, 5);
        assert_eq!(view["epic:hades"].seconds, 300);
    }

    #[test]
    fn a_kept_entry_never_played_itself_still_shows_its_alias_time() {
        let map = HashMap::from([("windows:hades".to_string(), stat(&[(0, 90)]))]);
        let aliases = HashMap::from([("steam:1".to_string(), vec!["windows:hades".to_string()])]);
        assert_eq!(with_aliases(map, &aliases)["steam:1"].seconds, 90);
    }

    #[test]
    fn merged_history_keeps_the_newest_sessions_and_every_second() {
        let a = stat(&(0..HISTORY_MAX as u64).map(|i| (i * 10, i * 10 + 1)).collect::<Vec<_>>());
        let b = stat(&[(1_000_000, 1_000_100)]);
        let merged = merge_stats(&[&a, &b]);
        assert_eq!(merged.history.len(), HISTORY_MAX);
        assert_eq!(merged.history.last().map(|s| s.start), Some(1_000_000));
        assert_eq!(merged.history.first().map(|s| s.start), Some(10));
        assert_eq!(merged.seconds, HISTORY_MAX as u64 + 100);
    }

    #[test]
    fn the_alias_map_reports_changes_only() {
        let a = HashMap::from([("epic:x".to_string(), "steam:x".to_string())]);
        assert!(!set_aliases(&HashMap::new()));
        assert!(set_aliases(&a));
        assert!(!set_aliases(&a));
        assert!(set_aliases(&HashMap::new()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `procs` list shaped exactly like `running_processes` returns it:
    /// `(pid, lowercased full executable path)`.
    fn procs(entries: &[(u32, &str)]) -> Vec<(u32, String)> {
        entries.iter().map(|(pid, path)| (*pid, path.to_lowercase())).collect()
    }

    #[test]
    fn sessions_recorded_before_the_summary_existed_still_load() {
        let old: PlayStat = serde_json::from_str(
            r#"{"seconds":3600,"last_played":2,"history":[{"start":1,"end":2}]}"#,
        )
        .expect("old playtime record");
        assert_eq!(old.history.len(), 1);
        assert!(old.history[0].perf.is_none());
    }

    #[test]
    fn a_session_nothing_was_measured_in_is_stored_as_before() {
        let mut stat = PlayStat::default();
        push_session(&mut stat, 100, 400, None);
        assert_eq!(stat.seconds, 300);
        assert_eq!(stat.last_played, Some(400));
        let json = serde_json::to_string(&stat.history).expect("history");
        assert_eq!(json, r#"[{"start":100,"end":400}]"#);
    }

    #[test]
    fn a_measured_session_keeps_its_summary_and_the_history_stays_capped() {
        let perf = SessionPerf {
            avg_fps: Some(143.3),
            low_1_fps: Some(98.0),
            fps_secs: Some(1800),
            max_gpu_temp_c: Some(74),
            max_cpu_temp_c: None,
        };
        let mut stat = PlayStat::default();
        for i in 0..(HISTORY_MAX as u64 + 5) {
            push_session(&mut stat, i * 10, i * 10 + 5, None);
        }
        push_session(&mut stat, 90_000, 90_600, Some(perf.clone()));
        assert_eq!(stat.history.len(), HISTORY_MAX);
        // Dropped sessions stay counted in the total.
        assert_eq!(stat.seconds, (HISTORY_MAX as u64 + 5) * 5 + 600);
        let back: Vec<Session> =
            serde_json::from_str(&serde_json::to_string(&stat.history).expect("history"))
                .expect("round trip");
        assert_eq!(back.last().and_then(|s| s.perf.clone()), Some(perf));
    }

    #[test]
    fn exact_executable_path_matches_regardless_of_case() {
        let p = procs(&[(7, r"C:\Windows\explorer.exe"), (10, r"C:\Games\Foo\foo.exe")]);
        assert_eq!(find_pid(&p, None, Some(r"C:\GAMES\Foo\FOO.exe"), 0), Some(10));
    }

    #[test]
    fn install_dir_prefix_matches_when_the_executable_is_unknown() {
        let p = procs(&[(7, r"C:\Windows\explorer.exe"), (42, r"C:\Games\Foo\bin\foo.exe")]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 0), Some(42));
    }

    #[test]
    fn helper_processes_under_the_install_dir_are_excluded() {
        let p = procs(&[
            (11, r"C:\Games\Foo\UnityCrashHandler64.exe"),
            (12, r"C:\Games\Foo\_CommonRedist\vc_redist.x64.exe"),
            (13, r"C:\Games\Foo\dxsetup.exe"),
            (14, r"C:\Games\Foo\bin\foo.exe"),
        ]);
        // The three helpers are skipped and the real executable wins.
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 0), Some(14));
    }

    #[test]
    fn anti_cheat_services_are_excluded_by_their_directory() {
        // Neither file name says "anti-cheat"; the directory does, which is why the
        // whole relative path is matched instead of just the file name.
        let p = procs(&[
            (11, r"C:\Games\Foo\EasyAntiCheat\EasyAntiCheat.exe"),
            (12, r"C:\Games\Foo\BattlEye\BEService.exe"),
            (13, r"C:\Games\Foo\bin\foo.exe"),
        ]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 0), Some(13));
    }

    #[test]
    fn a_trailing_separator_on_the_install_dir_is_tolerated() {
        let p = procs(&[(42, r"C:\Games\Foo\bin\foo.exe")]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo\"), None, 0), Some(42));
    }

    #[test]
    fn the_install_dir_itself_is_not_a_match() {
        // Only entries *inside* the directory count; the directory path on its own
        // has no process behind it.
        let p = procs(&[(42, r"C:\Games\Foo")]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 0), None);
    }

    #[test]
    fn an_empty_install_dir_never_matches() {
        // Without the `is_empty` guard every running process would prefix-match "",
        // so any game with no InstallLocation would look permanently running.
        let p = procs(&[(7, r"C:\Windows\explorer.exe")]);
        assert_eq!(find_pid(&p, Some(""), None, 0), None);
        assert_eq!(find_pid(&p, Some(""), Some(""), 0), None);
    }

    #[test]
    fn returns_none_when_nothing_matches() {
        let p = procs(&[(7, r"C:\Windows\explorer.exe")]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), Some(r"C:\Games\Foo\foo.exe"), 0), None);
        assert_eq!(find_pid(&[], Some(r"C:\Games\Foo"), Some(r"C:\Games\Foo\foo.exe"), 0), None);
    }

    // --- Regression tests for the three matching defects fixed on 2026-09-08. ---

    #[test]
    fn a_sibling_directory_sharing_a_path_prefix_is_not_matched() {
        // The old `path.starts_with(dir)` had no separator boundary, so an install
        // dir of "C:\Games\Foo" claimed everything under "C:\Games\FooBar" and the
        // playtime clock and HUD attached to a different game.
        let p = procs(&[(99, r"C:\Games\FooBar\bin\foobar.exe")]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 0), None);
    }

    #[test]
    fn an_exact_executable_match_wins_over_an_earlier_install_dir_hit() {
        // Both checks used to share one pass over the process list, so enumeration
        // order decided: a loose directory hit first in the list beat the exact
        // executable behind it.
        let p = procs(&[
            (20, r"C:\Games\Foo\bin\helper_ui.exe"),
            (21, r"C:\Games\Foo\bin\foo.exe"),
        ]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), Some(r"C:\Games\Foo\bin\foo.exe"), 0), Some(21));
    }

    #[test]
    fn the_anti_cheat_exclusions_match_the_processes_that_actually_ship() {
        // Both entries used to be dead: "easanticheat" never matched the shipped
        // "EasyAntiCheat.exe" (the 'y' breaks the substring), and "battleye" was
        // tested against the file name "BEService.exe" instead of the directory.
        let eac = procs(&[(30, r"C:\Games\Foo\EasyAntiCheat\EasyAntiCheat.exe")]);
        assert_eq!(find_pid(&eac, Some(r"C:\Games\Foo"), None, 0), None);

        let be = procs(&[(31, r"C:\Games\Foo\BattlEye\BEService.exe")]);
        assert_eq!(find_pid(&be, Some(r"C:\Games\Foo"), None, 0), None);
    }

    // --- MT1 / P1 (2026-09-17). ---

    #[test]
    fn the_foreground_game_wins_over_a_launcher_started_before_it() {
        // Regression (MT1): with no known executable the first process under the
        // install dir won, so the launcher became the HUD/PresentMon target.
        let p = procs(&[
            (40, r"C:\Games\Foo\launcher.exe"),
            (41, r"C:\Games\Foo\bin\game.exe"),
        ]);
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 41), Some(41));
        // No foreground information: snapshot order, as before.
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 0), Some(40));
        // Foreground is another app entirely: it is not adopted.
        assert_eq!(find_pid(&p, Some(r"C:\Games\Foo"), None, 7), Some(40));
    }

    #[test]
    fn a_foreground_install_dir_process_never_beats_the_exact_executable() {
        let p = procs(&[
            (50, r"C:\Games\Foo\bin\foo.exe"),
            (51, r"C:\Games\Foo\launcher.exe"),
        ]);
        assert_eq!(
            find_pid(&p, Some(r"C:\Games\Foo"), Some(r"C:\Games\Foo\bin\foo.exe"), 51),
            Some(50)
        );
    }

    #[test]
    fn the_tracked_pid_follows_the_foreground_between_full_scans() {
        let dir = Some(r"C:\Games\Foo");
        let launcher = r"c:\games\foo\launcher.exe";
        let game = r"c:\games\foo\bin\game.exe";
        assert!(prefer_foreground(Some(launcher), game, dir, None));
        assert!(!prefer_foreground(Some(launcher), r"c:\windows\explorer.exe", dir, None));
        assert!(!prefer_foreground(Some(launcher), r"c:\games\foo\easyanticheat\easyanticheat.exe", dir, None));
        // An exact-executable pid is kept against a mere install-dir hit…
        let exe = Some(r"C:\Games\Foo\bin\game.exe");
        assert!(!prefer_foreground(Some(game), launcher, dir, exe));
        // …but a foreground exact match is always adopted.
        assert!(prefer_foreground(Some(launcher), game, dir, exe));
    }

    #[test]
    fn active_sessions_are_persisted_on_change_or_every_interval_not_every_poll() {
        // Regression (P1): `last_seen` moves every 5 s poll, so the store was
        // rewritten with an fsync on every tick for the whole play session.
        let one = vec![("steam:1".to_string(), 1_000)];
        assert!(should_persist_active(&[], &one, None, 1_000));
        assert!(!should_persist_active(&one, &one, Some(1_000), 1_005));
        assert!(!should_persist_active(&one, &one, Some(1_000), 1_000 + ACTIVE_PERSIST_SECS - 1));
        assert!(should_persist_active(&one, &one, Some(1_000), 1_000 + ACTIVE_PERSIST_SECS));
        // A session ending is written at once, whatever the interval says.
        assert!(should_persist_active(&one, &[], Some(1_000), 1_005));
    }
    fn entry(id: &str, source: crate::models::GameSource, dir: Option<&str>, exe: Option<&str>) -> IndexEntry {
        IndexEntry {
            id: id.to_string(),
            name: id.to_string(),
            install_dir: dir.map(str::to_string),
            executable: exe.map(str::to_string),
            source,
        }
    }

    #[test]
    fn a_game_started_outside_astrail_is_matched_by_its_foreground_process() {
        use crate::models::GameSource::{Gog, Steam};
        let index = vec![
            entry("steam:10", Steam, Some(r"D:\SteamLibrary\steamapps\common\Foo"), None),
            entry("gog:20", Gog, Some(r"C:\GOG Games\Bar"), Some(r"C:\GOG Games\Bar\bin\Bar.exe")),
        ];
        let hit = |path: &str| match_foreground(&index, path).map(|e| e.id.as_str());
        assert_eq!(hit(r"d:\steamlibrary\steamapps\common\foo\foo.exe"), Some("steam:10"));
        assert_eq!(hit(r"c:\gog games\bar\bin\bar.exe"), Some("gog:20"));
        // Anything else in front (a browser, the store client) is nobody's game.
        assert_eq!(hit(r"c:\program files\mozilla firefox\firefox.exe"), None);
        assert_eq!(hit(r"c:\program files (x86)\steam\steam.exe"), None);
        // A sibling that only shares a textual prefix is not inside the install dir.
        assert_eq!(hit(r"d:\steamlibrary\steamapps\common\foobar\foobar.exe"), None);
        // Helpers under the install dir never start a session.
        assert_eq!(hit(r"d:\steamlibrary\steamapps\common\foo\easyanticheat\easyanticheat.exe"), None);
        assert_eq!(hit(r"d:\steamlibrary\steamapps\common\foo\unitycrashhandler64.exe"), None);
    }

    #[test]
    fn an_application_in_front_is_never_a_play_session() {
        // Browsers and tools are in the library too. Focusing one must not start a
        // session (or put the HUD over it); they are only timed when launched here.
        use crate::models::GameSource::{App, Steam};
        let index = vec![
            entry("app:firefox", App, Some(r"C:\Program Files\Mozilla Firefox"), Some(r"C:\Program Files\Mozilla Firefox\firefox.exe")),
            // The Steam client as an app: its dir contains every game of the default library.
            entry("app:steam", App, Some(r"C:\Program Files (x86)\Steam"), None),
            entry("steam:10", Steam, Some(r"C:\Program Files (x86)\Steam\steamapps\common\Foo"), None),
        ];
        let hit = |path: &str| {
            match_foreground(&index, path)
                .filter(|e| tracked_when_started_outside(&e.source, || true))
                .map(|e| e.id.as_str())
        };
        assert_eq!(hit(r"c:\program files\mozilla firefox\firefox.exe"), None);
        assert_eq!(hit(r"c:\program files (x86)\steam\steam.exe"), None);
        assert_eq!(hit(r"c:\program files (x86)\steam\steamapps\common\foo\foo.exe"), Some("steam:10"));
    }

    #[test]
    fn a_tool_inside_a_game_folder_is_not_the_game() {
        // The application owns its process: the game around it must not claim it.
        use crate::models::GameSource::{App, Steam};
        let index = vec![
            entry("steam:10", Steam, Some(r"D:\Games\Foo"), None),
            entry("app:modtool", App, Some(r"D:\Games\Foo\ModTool"), None),
        ];
        let owner = match_foreground(&index, r"d:\games\foo\modtool\modtool.exe").map(|e| e.id.as_str());
        assert_eq!(owner, Some("app:modtool"));
    }

    #[test]
    fn a_registry_guess_needs_the_user_to_confirm_it_is_a_game() {
        // `Windows` means "the application list did not know it", which is also true
        // of every unknown tool. Focusing one must not time a session, draw the HUD
        // and publish a Discord status.
        use crate::models::GameSource::{App, Manual, Steam, Windows};
        assert!(!tracked_when_started_outside(&Windows, || false));
        assert!(tracked_when_started_outside(&Windows, || true));
        assert!(!tracked_when_started_outside(&App, || true));
        // Store games and hand-added entries never need the override, so its file is
        // not even read for them.
        assert!(tracked_when_started_outside(&Steam, || unreachable!("not consulted")));
        assert!(tracked_when_started_outside(&Manual, || unreachable!("not consulted")));

        // The user's word: the explicit override, or having played it from Astrail.
        let played = PlayStat { seconds: 60, last_played: Some(1), history: Vec::new() };
        let never = PlayStat { seconds: 0, last_played: None, history: Vec::new() };
        assert!(is_confirmed_game(Some("game"), None));
        assert!(is_confirmed_game(None, Some(&played)));
        assert!(!is_confirmed_game(None, Some(&never)));
        assert!(!is_confirmed_game(Some("app"), None));
        assert!(!is_confirmed_game(None, None));
    }

    #[test]
    fn nested_install_dirs_resolve_to_the_deepest_and_the_exact_executable_wins() {
        use crate::models::GameSource::{Steam, Windows};
        let index = vec![
            // A registry entry whose "install location" is the folder holding several games.
            entry("windows:pack", Windows, Some(r"D:\Games"), None),
            entry("steam:10", Steam, Some(r"D:\Games\Foo"), None),
            entry("windows:tool", Windows, Some(r"D:\Games\Foo\Tools"), Some(r"D:\Games\Foo\Tools\Editor.exe")),
        ];
        let hit = |path: &str| match_foreground(&index, path).map(|e| e.id.as_str());
        assert_eq!(hit(r"d:\games\foo\foo.exe"), Some("steam:10"));
        assert_eq!(hit(r"d:\games\other\other.exe"), Some("windows:pack"));
        assert_eq!(hit(r"d:\games\foo\tools\editor.exe"), Some("windows:tool"));
        // Whatever is matched here must be found again by the scan, or the launch
        // stays pending and forces full scans for two minutes.
        let procs = procs(&[(7, r"D:\Games\Foo\Foo.exe")]);
        assert_eq!(find_pid(&procs, Some(r"D:\Games\Foo"), None, 7), Some(7));
    }

    #[test]
    fn a_drive_root_never_claims_a_foreground_process() {
        use crate::models::GameSource::Windows;
        let index = vec![
            entry("windows:root", Windows, Some(r"D:\"), None),
            entry("windows:bare", Windows, Some("D:"), None),
            entry("windows:empty", Windows, Some(""), None),
        ];
        assert!(match_foreground(&index, r"d:\anything\tool.exe").is_none());
        assert!(is_specific_dir(r"d:\witcher3"));
        assert!(!is_specific_dir(r"d:\"));
    }

    #[test]
    fn an_external_game_is_registered_once() {
        let id = "test:registered-once";
        assert!(register_external(id));
        assert!(!register_external(id));
        LAUNCHED_FROM_ASTRAIL
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(i, _)| i != id);
    }
}
