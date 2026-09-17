//! FPS / frametime via **PresentMon** (Intel/Microsoft, ETW-based — no DLL
//! injection, so anti-cheat safe). A controller thread spawns `PresentMon.exe`
//! targeting the running game's PID, streams its CSV from stdout, and keeps a
//! ~1s rolling window of frame times to derive FPS and average frametime.
//!
//! Requirements (both needed for FPS to appear; everything degrades silently to
//! `None` otherwise, so the rest of the overlay always works):
//!   1. The `PresentMon.exe` binary present (see `binaries/README.md`).
//!   2. Meteor running **elevated** — ETW realtime sessions require admin.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Latest FPS / frametime as hundredths (0 = no data), so they fit in atomics.
static FPS_X100: AtomicU32 = AtomicU32::new(0);
static FRAMETIME_X100: AtomicU32 = AtomicU32::new(0);
/// `metrics::clock_ms()` of the last published frame (0 = never).
static LAST_UPDATE_MS: AtomicU64 = AtomicU64::new(0);

/// Our own ETW session name. PresentMon defaults to a fixed well-known name, which
/// is why `--stop_existing_session` used to be needed — and why it could tear down
/// a session belonging to another PresentMon consumer (CapFrameX, Intel's own
/// service, the user's own run). With a private name we only ever stop our own.
const SESSION_NAME: &str = "Meteor-PresentMon";

/// The running child, shared with `shutdown()` so a clean app exit can stop the
/// ETW session instead of leaving the Job Object to terminate the process.
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// Ceiling for a graceful stop before we terminate and fall back to stopping the
/// ETW session ourselves.
const GRACEFUL_STOP: Duration = Duration::from_millis(1500);

fn child_lock() -> MutexGuard<'static, Option<Child>> {
    CHILD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Current FPS and average frametime (ms), if PresentMon produced them recently.
///
/// The reader only writes when a frame arrives, so a game that stops presenting
/// (loading screen, hang, minimized) must expire here instead of freezing its last
/// FPS on the HUD.
pub fn current() -> (Option<f32>, Option<f32>) {
    let stamp = LAST_UPDATE_MS.load(Ordering::Relaxed);
    if !crate::metrics::is_fresh(stamp, crate::metrics::clock_ms(), crate::metrics::fps_max_age_ms()) {
        return (None, None);
    }
    let f = FPS_X100.load(Ordering::Relaxed);
    let ft = FRAMETIME_X100.load(Ordering::Relaxed);
    let opt = |v: u32| if v == 0 { None } else { Some(v as f32 / 100.0) };
    (opt(f), opt(ft))
}

fn reset() {
    FPS_X100.store(0, Ordering::Relaxed);
    FRAMETIME_X100.store(0, Ordering::Relaxed);
    LAST_UPDATE_MS.store(0, Ordering::Relaxed);
}

/// Locate the PresentMon binary: bundled resource, next to our exe, or the dev
/// `binaries/` folder (cwd is `src-tauri` under `tauri dev`).
fn find_binary(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("binaries/PresentMon.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("PresentMon.exe"));
        }
    }
    candidates.push(PathBuf::from("binaries/PresentMon.exe"));
    candidates.into_iter().find(|p| p.exists())
}

/// Spawn PresentMon for a PID, with a reader thread parsing its stdout CSV.
fn spawn(bin: &Path, pid: u32) -> std::io::Result<Child> {
    use crate::sidecar_integrity::{open_verified, PRESENTMON_SHA256};
    // Held until the process exists: see `sidecar_integrity`.
    let _pinned = open_verified(bin, PRESENTMON_SHA256)?;
    // PresentMon 2.x uses GNU-style `--` flags. `--v1_metrics` keeps the stable
    // `msBetweenPresents` column (frametime) the parser looks for.
    let mut cmd = Command::new(bin);
    cmd.args([
        "--process_id",
        &pid.to_string(),
        "--output_stdout",
        // Private session name + stop-existing scoped to it: a leftover session of
        // ours from a previous run is cleaned up, another application's is not.
        "--session_name",
        SESSION_NAME,
        "--stop_existing_session",
        "--no_console_stats",
        "--terminate_on_proc_exit",
        "--v1_metrics",
    ]);
    // stdin is piped and held open so dropping it can ask PresentMon to stop and
    // close its ETW session; `stop` still verifies and forces the session down.
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    // Kill-on-close job: if Meteor dies for any reason (crash, force-quit,
    // `panic = "abort"`), the kernel terminates this elevated child and its ETW
    // session instead of leaving it orphaned.
    #[cfg(windows)]
    crate::jobobj::assign(&child);
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || parse_stdout(out));
    }
    Ok(child)
}

/// Frames kept per swapchain: roughly the last second of presents.
const WINDOW_MS: f32 = 1000.0;
/// A swapchain that has not presented for this long no longer competes for "busiest".
const CHAIN_TTL_MS: u64 = 1000;
/// Upper bound on tracked swapchains, so a process that churns swapchains cannot
/// grow the table without limit.
const MAX_CHAINS: usize = 8;

/// Rolling ~1 s window of one swapchain's frame times.
struct Chain {
    id: u64,
    frames: VecDeque<f32>,
    sum: f32,
    last_ms: u64,
}

/// Parses PresentMon's CSV and keeps one frame window **per swapchain**.
///
/// A game process often owns more than one swapchain (an embedded Chromium
/// launcher/UI, a secondary window, a video layer). PresentMon emits one row per
/// present for all of them, and `msBetweenPresents` is measured per swapchain, so
/// folding every row into one window mixed a 144 fps game with a 30 fps UI into a
/// single inflated number. The reported value is the busiest live swapchain, which
/// is the one rendering the game.
pub(crate) struct FrameParser {
    ft_col: Option<usize>,
    sc_col: Option<usize>,
    chains: Vec<Chain>,
}

impl FrameParser {
    pub(crate) fn new() -> Self {
        Self { ft_col: None, sc_col: None, chains: Vec::new() }
    }

    /// Feed one CSV line read at `now_ms`. Returns `(fps, avg_frametime_ms)` of the
    /// busiest live swapchain after a valid data row, `None` otherwise.
    pub(crate) fn feed(&mut self, line: &str, now_ms: u64) -> Option<(f32, f32)> {
        // Header: locate the frametime column (the name varies across versions:
        // "msBetweenPresents" / "MsBetweenPresents") and the swapchain column. Parsed
        // once; the swapchain column is optional (all rows share one window without it).
        let Some(ft_idx) = self.ft_col else {
            for (i, c) in line.split(',').enumerate() {
                let c = c.trim().to_ascii_lowercase();
                if c.contains("betweenpresents") {
                    self.ft_col = Some(i);
                } else if c == "swapchainaddress" {
                    self.sc_col = Some(i);
                }
            }
            return None;
        };

        // Data rows (the hot path at 200-800 fps): one pass over the fields, no
        // per-line allocation.
        let mut ft: Option<f32> = None;
        let mut chain_id: u64 = 0;
        for (i, field) in line.split(',').enumerate() {
            if i == ft_idx {
                ft = field.trim().parse::<f32>().ok();
            } else if Some(i) == self.sc_col {
                chain_id = parse_address(field);
            }
        }
        let ft = ft.filter(|v| v.is_finite() && *v > 0.0)?;

        self.chains.retain(|c| now_ms.saturating_sub(c.last_ms) <= CHAIN_TTL_MS);
        let pos = match self.chains.iter().position(|c| c.id == chain_id) {
            Some(p) => p,
            None => {
                if self.chains.len() >= MAX_CHAINS {
                    if let Some(oldest) = (0..self.chains.len()).min_by_key(|&i| self.chains[i].last_ms) {
                        self.chains.swap_remove(oldest);
                    }
                }
                self.chains.push(Chain { id: chain_id, frames: VecDeque::new(), sum: 0.0, last_ms: now_ms });
                self.chains.len() - 1
            }
        };

        let chain = &mut self.chains[pos];
        chain.last_ms = now_ms;
        chain.frames.push_back(ft);
        chain.sum += ft;
        while chain.sum > WINDOW_MS && chain.frames.len() > 1 {
            if let Some(old) = chain.frames.pop_front() {
                chain.sum -= old;
            }
        }

        // Busiest = most presents in its last second; ties go to the most recent.
        let best = self.chains.iter().max_by_key(|c| (c.frames.len(), c.last_ms))?;
        let avg_ft = best.sum / best.frames.len() as f32;
        (avg_ft > 0.0).then(|| (1000.0 / avg_ft, avg_ft))
    }
}

/// `0x0000020F3A1B2C40` → its numeric value; anything unparsable maps to 0, which
/// just groups those rows into one shared window.
fn parse_address(field: &str) -> u64 {
    let f = field.trim();
    let hex = f.strip_prefix("0x").or_else(|| f.strip_prefix("0X")).unwrap_or(f);
    u64::from_str_radix(hex, 16).unwrap_or(0)
}

/// Read PresentMon's CSV stream and publish the busiest swapchain's FPS/frametime.
fn parse_stdout(out: impl std::io::Read) {
    let mut reader = BufReader::new(out);
    let mut parser = FrameParser::new();

    // One reused buffer instead of `lines()`, which hands back an owned `String` per
    // line: this reads one line per presented frame, so at the 200-800 fps this path
    // exists for that was 200-800 heap allocations per second, sustained for the
    // whole play session — the most frequent allocation site in the app.
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // EOF.
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let now = crate::metrics::clock_ms();
        if let Some((fps, avg_ft)) = parser.feed(line.trim_end(), now) {
            FRAMETIME_X100.store((avg_ft * 100.0) as u32, Ordering::Relaxed);
            FPS_X100.store((fps * 100.0) as u32, Ordering::Relaxed);
            LAST_UPDATE_MS.store(now, Ordering::Relaxed);
        }
    }
    // Stream ended (game closed / PresentMon stopped): clear stale numbers.
    reset();
}

/// Force our ETW realtime session down.
///
/// An ETW realtime logger is a kernel object created by `StartTrace`: it outlives
/// the process that created it, so a terminated PresentMon leaves the session live
/// with its buffers pinned until reboot. Windows also caps how many loggers can
/// exist at once, so leaked sessions accumulate into a hard failure.
/// `EVENT_TRACE_CONTROL_STOP` by name is the only way to guarantee it is gone.
#[cfg(windows)]
fn stop_etw_session() {
    use windows::core::HSTRING;
    use windows::Win32::System::Diagnostics::Etw::{
        ControlTraceW, CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_PROPERTIES,
    };

    /// `EVENT_TRACE_PROPERTIES` is a header immediately followed by the logger-name
    /// and log-file-name buffers the API writes back into. Expressing that as one
    /// `#[repr(C)]` allocation keeps both the layout and the alignment right — a
    /// `Vec<u8>` scratch buffer would only be 1-byte aligned.
    #[repr(C)]
    struct TraceProps {
        props: EVENT_TRACE_PROPERTIES,
        logger_name: [u16; 256],
        log_file_name: [u16; 256],
    }

    const NAME_BYTES: u32 = 256 * 2;
    let header = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;

    // SAFETY: EVENT_TRACE_PROPERTIES and two u16 arrays are plain data with no
    // niches or invalid bit patterns, so an all-zero value is a valid instance.
    let mut p: TraceProps = unsafe { std::mem::zeroed() };
    p.props.Wnode.BufferSize = std::mem::size_of::<TraceProps>() as u32;
    p.props.LoggerNameOffset = header;
    p.props.LogFileNameOffset = header + NAME_BYTES;

    let name = HSTRING::from(SESSION_NAME);
    // SAFETY: `p` is a single #[repr(C)] allocation whose true size is declared in
    // `Wnode.BufferSize`, with both name offsets pointing inside it — the contract
    // ControlTraceW documents for stopping a session by name.
    let status = unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            &name,
            &mut p.props,
            EVENT_TRACE_CONTROL_STOP,
        )
    };

    // 0 = stopped. 4201 (ERROR_WMI_INSTANCE_NOT_FOUND) = already gone, which is the
    // expected result when PresentMon shut itself down cleanly.
    if status.0 != 0 && status.0 != 4201 {
        eprintln!(
            "could not stop the {SESSION_NAME} ETW session (error {})",
            status.0
        );
    }
}

#[cfg(not(windows))]
fn stop_etw_session() {}

/// Stop PresentMon and make sure its ETW session goes with it.
///
/// Dropping stdin asks it to stop on its own; `kill()` is TerminateProcess, which
/// leaves the realtime session behind. The explicit session stop runs either way,
/// because a clean exit is not something we can verify from here.
fn stop(mut child: Child) {
    drop(child.stdin.take());

    let deadline = Instant::now() + GRACEFUL_STOP;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    stop_etw_session();
}

/// Stop PresentMon on application exit, before the Job Object terminates it and
/// strands its ETW session.
pub fn shutdown() {
    let child = child_lock().take();
    if let Some(child) = child {
        stop(child);
        reset();
    }
}

/// Start the PresentMon controller thread. Idle until the overlay wants FPS and a
/// game is running; it (re)targets PresentMon at the current game's PID.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // PresentMon's ETW realtime session requires admin. Elevation can't change at
        // runtime, so check once: when not elevated we never even attempt to spawn it
        // (no access-denied spam, no overhead). FPS on NVIDIA therefore only appears
        // when Meteor is already running as admin; AMD gets FPS from ADLX regardless.
        let elevated = {
            #[cfg(windows)]
            {
                crate::elevation::is_elevated()
            }
            #[cfg(not(windows))]
            {
                false
            }
        };
        if !elevated {
            return;
        }
        let mut child_pid: u32 = 0;
        // Once we fail to find the binary, stop retrying every tick (logged once).
        let mut bin_missing_logged = false;
        // PID we already failed to attach to (no admin → ETW access denied, or no
        // binary). Without this we'd respawn PresentMon.exe every 500ms for the whole
        // session — a real hitch source for users without elevation (esp. NVIDIA,
        // where PresentMon is the only FPS source). Cleared when the target changes.
        let mut failed_pid: u32 = 0;
        let mut seen: u64 = 0;

        loop {
            // Park while there is nothing to target; poll only while a session is
            // live, so an idle Meteor does not wake this thread at all.
            let idle = !crate::metrics::want_fps() || crate::metrics::current_pid() == 0;
            let running = child_lock().is_some();
            crate::metrics::wait_sidecar(
                &mut seen,
                (!idle || running).then(|| Duration::from_millis(500)),
            );

            let want_pid = if crate::metrics::want_fps() {
                crate::metrics::current_pid()
            } else {
                0
            };

            // A new target clears the previous failure so the new game gets a try.
            if want_pid != failed_pid {
                failed_pid = 0;
            }

            // Target changed (new game / stopped): tear down the old instance. Skip
            // re-attempting a PID we already failed on (failed_pid) to avoid respawning.
            if want_pid != child_pid && want_pid != failed_pid {
                // Take the child out before stopping it: `stop` waits up to
                // GRACEFUL_STOP and must not hold the lock while it does.
                let old = child_lock().take();
                if let Some(old) = old {
                    stop(old);
                }
                reset();
                child_pid = 0;

                if want_pid != 0 {
                    match find_binary(&app) {
                        Some(bin) => match spawn(&bin, want_pid) {
                            Ok(c) => {
                                *child_lock() = Some(c);
                                child_pid = want_pid;
                            }
                            Err(e) => {
                                // Typically "access denied" without elevation. Mark the
                                // PID failed so we don't hammer respawns every tick.
                                eprintln!("PresentMon no pudo iniciarse: {e}");
                                failed_pid = want_pid;
                            }
                        },
                        None => {
                            // No binary: don't re-scan the filesystem every tick either.
                            failed_pid = want_pid;
                            if !bin_missing_logged {
                                eprintln!(
                                    "PresentMon.exe no encontrado: FPS/frametime deshabilitados."
                                );
                                bin_missing_logged = true;
                            }
                        }
                    }
                }
            }

            // Reap a child that exited on its own (game closed, ETW denied, …).
            let exited = matches!(
                child_lock().as_mut().map(|c| c.try_wait()),
                Some(Ok(Some(_)))
            );
            if exited {
                let dead = child_pid;
                *child_lock() = None;
                child_pid = 0;
                reset();
                // PresentMon exiting on its own does not guarantee it closed the
                // realtime session (it may have been killed, or died mid-startup).
                stop_etw_session();
                // If the game is still running, PresentMon died by itself (e.g. ETW
                // denied at runtime) — mark the PID failed so we don't respawn every
                // tick. If the game closed (want_pid changed), this is just cleanup.
                if dead != 0 && want_pid == dead {
                    failed_pid = dead;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,Dropped,TimeInSeconds,msInPresentAPI,msBetweenPresents";

    fn row(chain: &str, ft: f32) -> String {
        format!("game.exe,1234,{chain},DXGI,0,0,0,1.0,0.1,{ft}")
    }

    #[test]
    fn single_swapchain_reports_its_rate() {
        let mut p = FrameParser::new();
        assert_eq!(p.feed(HEADER, 1), None);
        let mut last = None;
        for i in 0..120 {
            last = p.feed(&row("0x00000001", 1000.0 / 60.0), 1 + i * 16);
        }
        let (fps, ft) = last.expect("data rows publish a value");
        assert!((fps - 60.0).abs() < 0.5, "fps {fps}");
        assert!((ft - 16.667).abs() < 0.1, "ft {ft}");
    }

    #[test]
    fn a_second_swapchain_does_not_inflate_fps() {
        // Regression (MT3): a 144 fps game plus a 30 fps embedded UI in the same
        // process used to be folded into one window and read as ~174 fps.
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        let mut last = None;
        let mut t_game = 0.0f32;
        let mut t_ui = 0.0f32;
        let (ft_game, ft_ui) = (1000.0 / 144.0, 1000.0 / 30.0);
        while t_game < 3000.0 {
            if t_ui <= t_game {
                last = p.feed(&row("0x000002AA", ft_ui), 1 + t_ui as u64);
                t_ui += ft_ui;
            } else {
                last = p.feed(&row("0x000001BB", ft_game), 1 + t_game as u64);
                t_game += ft_game;
            }
        }
        let (fps, _) = last.expect("value");
        assert!((fps - 144.0).abs() < 1.0, "fps {fps}");
    }

    #[test]
    fn a_swapchain_that_stops_presenting_stops_competing() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        // Busy chain for one second, then silent.
        for i in 0..144u64 {
            p.feed(&row("0x1", 1000.0 / 144.0), 1 + i * 7);
        }
        // Slow chain keeps going past the busy chain's TTL.
        let mut last = None;
        for i in 0..20u64 {
            last = p.feed(&row("0x2", 100.0), 1_100 + i * 100);
        }
        let (fps, _) = last.expect("value");
        assert!((fps - 10.0).abs() < 0.5, "fps {fps}");
    }

    #[test]
    fn rows_without_a_swapchain_column_share_one_window() {
        let mut p = FrameParser::new();
        p.feed("Application,ProcessID,msBetweenPresents", 1);
        let mut last = None;
        for i in 0..60u64 {
            last = p.feed(&format!("game.exe,1234,{}", 1000.0 / 60.0), 1 + i * 16);
        }
        assert!((last.expect("value").0 - 60.0).abs() < 0.5);
    }

    #[test]
    fn garbage_and_non_positive_frametimes_are_ignored() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        assert_eq!(p.feed(&row("0x1", 0.0), 2), None);
        assert_eq!(p.feed("game.exe,1234,0x1,DXGI,0,0,0,1.0,0.1,NaN", 3), None);
        assert_eq!(p.feed("short,row", 4), None);
    }

    #[test]
    fn tracked_swapchains_are_bounded() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        for i in 0..100u64 {
            p.feed(&row(&format!("0x{i:X}"), 16.0), 1 + i);
        }
        assert!(p.chains.len() <= MAX_CHAINS);
    }

    #[test]
    fn swapchain_addresses_parse_with_or_without_prefix() {
        assert_eq!(parse_address(" 0x0000020F3A1B2C40 "), 0x0000_020F_3A1B_2C40);
        assert_eq!(parse_address("FF"), 0xFF);
        assert_eq!(parse_address("n/a"), 0);
    }
}
