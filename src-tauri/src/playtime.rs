// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::jsonstore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

static LAUNCHED_FROM_ASTRAIL: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// Wake signal for the watcher thread: a generation counter plus a condvar.
///
/// The watcher used to `sleep(5s)` forever and enumerate **every process on the
/// system** on each tick, even with nothing to track — and then discard all of
/// it, because matching is opt-in per launch (ADR-5). Now it blocks here with no
/// timeout while idle and is woken by `notify_launched`, so an idle Astrail does
/// no process work at all.
static WAKE: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

const STORE_FILE: &str = "playtime.json";
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
const HISTORY_MAX: usize = 500;
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

/// Play stats for a single game id (zeroed if never played).
pub fn get(app: &AppHandle, id: &str) -> PlayStat {
    load(app).get(id).cloned().unwrap_or_default()
}

/// Play stats for every game that has any (for sorting the whole library).
pub fn all(app: &AppHandle) -> HashMap<String, PlayStat> {
    load(app)
}

/// Persist one finished session for a game.
fn record_session(app: &AppHandle, id: &str, start: u64, end: u64) -> Result<(), String> {
    let seconds = end.saturating_sub(start);
    let mut map = load(app);
    let stat = map.entry(id.to_string()).or_default();
    stat.seconds += seconds;
    stat.last_played = Some(end);
    stat.history.push(Session { start, end });
    // Keep the newest `HISTORY_MAX`; the dropped ones stay counted in `seconds`.
    if stat.history.len() > HISTORY_MAX {
        let overflow = stat.history.len() - HISTORY_MAX;
        stat.history.drain(..overflow);
    }
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
        eprintln!("[playtime] could not save {ACTIVE_FILE}: {e}");
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
            let _ = record_session(app, &s.id, s.start, s.last_seen);
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

/// Start the global playtime watcher: a background thread that polls every
/// running process and matches them against the **whole library**, so a game is
/// timed no matter how it was launched (Astrail, Steam, a desktop shortcut…).
/// Sessions are accumulated per game id and the frontend is notified on end.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // id -> (start, last_seen, pid) for sessions currently in progress.
        let mut active: HashMap<String, (u64, u64, u32)> = HashMap::new();
        let mut index = library_index(&app);
        let mut index_mtime = library_cache_mtime(&app);
        // Seconds since the last full process enumeration. While a game is confirmed
        // running we only do cheap per-PID liveness checks between full scans, so the
        // watcher costs O(active games) instead of O(all processes) during play.
        let mut since_full = 0u64;
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

            // Re-read the index only when `get_library` actually rewrote it.
            let mtime = library_cache_mtime(&app);
            if mtime != index_mtime {
                index_mtime = mtime;
                index = library_index(&app);
            }

            // Full enumeration vs. cheap liveness. A full scan is needed while a
            // launch is still pending (we have no PID yet) and periodically to
            // notice a game that restarted itself; otherwise one syscall per
            // tracked PID is enough. Off-Windows there is no cheap liveness
            // primitive, so always scan.
            #[cfg(windows)]
            let mut do_full = pending_launch || since_full >= FULL_SCAN_SECS;
            #[cfg(not(windows))]
            let do_full = true;

            let foreground = crate::overlay::foreground_pid();
            let mut running: HashSet<String> = HashSet::new();

            #[cfg(windows)]
            if !do_full {
                since_full += POLL_SECS;
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
                since_full = 0;
                // (pid, lowercased exe path) captured once, reused for matching + pid.
                let procs = running_processes();

                for e in &index {
                    // OPT-IN: only games that are active or were launched via Astrail.
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
                    if last.saturating_sub(start) >= MIN_SESSION_SECS {
                        let _ = record_session(&app, &id, start, last);
                        let _ = app.emit("playtime-updated", &id);
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

            // Publish the foreground game (name + pid) to the metrics overlay
            // ONLY if it was launched from Astrail.
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
                eprintln!(
                    "[overlay] watcher: running_primary={:?} launched_from_astrail={} -> publish={:?} pid={:?}",
                    primary,
                    show_metrics_for.is_some(),
                    game_name,
                    game_pid
                );
                dbg_overlay_game = game_name.clone();
            }
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
mod tests {
    use super::*;

    /// Build a `procs` list shaped exactly like `running_processes` returns it:
    /// `(pid, lowercased full executable path)`.
    fn procs(entries: &[(u32, &str)]) -> Vec<(u32, String)> {
        entries.iter().map(|(pid, path)| (*pid, path.to_lowercase())).collect()
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
}
