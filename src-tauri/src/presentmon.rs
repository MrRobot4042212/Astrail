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

/// Read PresentMon's CSV stream and maintain a ~1s rolling window of frame times.
fn parse_stdout(out: impl std::io::Read) {
    let mut reader = BufReader::new(out);
    // Index of the "...BetweenPresents" column (frametime in ms), found from the header.
    let mut ft_col: Option<usize> = None;
    // Rolling window of recent frametimes (ms) and their running sum.
    let mut window: VecDeque<f32> = VecDeque::new();
    let mut sum = 0.0f32;

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
        let line = line.trim_end();

        // Header: locate the frametime column (name varies across versions:
        // "msBetweenPresents" / "MsBetweenPresents"). Parsed once. `split(',').position`
        // iterates without collecting into a Vec.
        if ft_col.is_none() {
            ft_col = line
                .split(',')
                .position(|c| c.trim().to_ascii_lowercase().contains("betweenpresents"));
            continue;
        }

        // `ft_col` is Some here — the branch above `continue`s otherwise — but this
        // runs on a reader thread where `panic = "abort"` would take the whole app
        // down, so the impossible case skips the line instead of asserting.
        let Some(idx) = ft_col else { continue };
        // Data rows (the hot path at 200-800 fps): pull just the Nth field with no
        // per-line allocation, instead of collecting every column into a Vec.
        let Some(ft) = line.split(',').nth(idx).and_then(|v| v.trim().parse::<f32>().ok()) else {
            continue;
        };
        if !(ft.is_finite() && ft > 0.0) {
            continue;
        }

        window.push_back(ft);
        sum += ft;
        // Keep roughly the last second of frames.
        while sum > 1000.0 && window.len() > 1 {
            if let Some(old) = window.pop_front() {
                sum -= old;
            }
        }

        let n = window.len() as f32;
        let avg_ft = sum / n;
        let fps = if avg_ft > 0.0 { 1000.0 / avg_ft } else { 0.0 };
        FRAMETIME_X100.store((avg_ft * 100.0) as u32, Ordering::Relaxed);
        FPS_X100.store((fps * 100.0) as u32, Ordering::Relaxed);
        LAST_UPDATE_MS.store(crate::metrics::clock_ms(), Ordering::Relaxed);
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
