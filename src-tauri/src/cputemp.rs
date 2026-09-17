//! CPU temperature via the LibreHardwareMonitor sidecar (`binaries/cputemp.exe`,
//! built from `sidecar/cputemp/`). LHM reads Ryzen Tctl / Intel core temps through
//! a kernel driver, so this needs **admin** and an HVCI-compatible driver; without
//! them the sidecar prints nothing and CPU temp degrades to `None` — same
//! best-effort contract as the PresentMon (FPS) integration.
//!
//! A controller thread runs the sidecar only while the overlay wants CPU temp and
//! a game is running, parses the one-int-per-line °C stream from its stdout, and
//! keeps the latest value in an atomic for the sampler.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Latest CPU temperature in °C (0 = no data).
static CPU_TEMP_C: AtomicU32 = AtomicU32::new(0);
/// `metrics::clock_ms()` of the last reading (0 = never).
static LAST_UPDATE_MS: AtomicU64 = AtomicU64::new(0);

/// The sidecar prints once a second; three missed lines mean it is wedged.
const MAX_AGE_MS: u64 = 3000;

/// After the sidecar exits on its own while still wanted (driver blocked by HVCI or
/// the vulnerable-driver blocklist, LHM crash), wait this long before respawning.
/// Without it the controller relaunched a 13 MB elevated .NET process every 500 ms
/// for the whole play session.
const RESPAWN_BACKOFF: Duration = Duration::from_secs(30);

/// The running sidecar, shared with `shutdown()` so a clean app exit can release
/// the kernel driver before the Job Object resorts to terminating the process.
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// How long to wait for the sidecar to release its driver and exit before killing
/// it. It wakes on stdin EOF, so the normal case is milliseconds; this is only the
/// ceiling for a sidecar wedged inside the driver.
const GRACEFUL_STOP: Duration = Duration::from_millis(1500);

fn child_lock() -> MutexGuard<'static, Option<Child>> {
    CHILD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Current CPU temperature, if the sidecar produced it recently.
pub fn current() -> Option<u32> {
    let stamp = LAST_UPDATE_MS.load(Ordering::Relaxed);
    if !crate::metrics::is_fresh(stamp, crate::metrics::clock_ms(), MAX_AGE_MS) {
        return None;
    }
    match CPU_TEMP_C.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v),
    }
}

fn reset() {
    CPU_TEMP_C.store(0, Ordering::Relaxed);
    LAST_UPDATE_MS.store(0, Ordering::Relaxed);
}

/// Parse one sidecar line: an integer °C, rejecting obviously bogus values.
fn parse_temp(line: &str) -> Option<u32> {
    line.trim().parse::<u32>().ok().filter(|v| *v > 0 && *v < 200)
}

/// Whether a sidecar that died on its own may be started again at `now`.
fn respawn_allowed(last_unexpected_exit: Option<Instant>, now: Instant) -> bool {
    last_unexpected_exit.is_none_or(|t| now.saturating_duration_since(t) >= RESPAWN_BACKOFF)
}

/// Locate the sidecar: bundled resource, next to our exe, or the dev `binaries/`.
fn find_binary(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("binaries/cputemp.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("cputemp.exe"));
        }
    }
    candidates.push(PathBuf::from("binaries/cputemp.exe"));
    candidates.into_iter().find(|p| p.exists())
}

/// Spawn the sidecar with a reader thread parsing its stdout (one °C int per line).
fn spawn(bin: &PathBuf) -> std::io::Result<Child> {
    use crate::sidecar_integrity::{open_verified, CPUTEMP_SHA256};
    // Held until the process exists: see `sidecar_integrity`.
    let _pinned = open_verified(bin, CPUTEMP_SHA256)?;
    let mut cmd = Command::new(bin);
    // stdin is piped and kept open on purpose: closing it is the sidecar's shutdown
    // signal, and the only way it ever unloads its kernel driver (see `stop`).
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
    // Kill-on-close job: last-resort backstop so an orphaned elevated sidecar cannot
    // outlive Meteor after a crash. Note it terminates rather than stops the child,
    // which does NOT unload the driver — that is what `stop` is for.
    #[cfg(windows)]
    crate::jobobj::assign(&child);
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(v) = parse_temp(&line) {
                    CPU_TEMP_C.store(v, Ordering::Relaxed);
                    LAST_UPDATE_MS.store(crate::metrics::clock_ms(), Ordering::Relaxed);
                }
            }
            // Stream ended (sidecar exited): clear the stale reading.
            reset();
        });
    }
    Ok(child)
}

/// Stop the sidecar so it unloads its kernel driver, killing it only if it refuses.
///
/// Dropping its stdin is the agreed shutdown signal: the sidecar sees EOF, calls
/// `computer.Close()` (which unloads the LibreHardwareMonitor driver) and exits.
/// `kill()` alone is TerminateProcess, which skips .NET finalizers and leaves the
/// driver loaded and registered for the rest of the boot — a documented local
/// privilege-escalation primitive and a kernel-anti-cheat blocklist trigger.
fn stop(mut child: Child) {
    drop(child.stdin.take());

    let deadline = Instant::now() + GRACEFUL_STOP;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            // Can't observe it any more; fall through to the kill.
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            eprintln!("cputemp did not stop within {GRACEFUL_STOP:?}; terminating (its driver may stay loaded)");
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let _ = child.kill();
    let _ = child.wait();
}

/// Stop the sidecar on application exit. Called from `RunEvent::Exit`, where the
/// Job Object would otherwise terminate it and strand the driver.
pub fn shutdown() {
    let child = child_lock().take();
    if let Some(child) = child {
        stop(child);
        reset();
    }
}

/// Start the controller thread. Idle until the overlay wants CPU temp and a game
/// is running; tears the sidecar down (unloading its driver) otherwise.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // The sidecar needs admin to load its kernel driver. Elevation cannot
        // change while we run, so check once instead of waking twice a second
        // for a process that could never start (this mirrors what the PresentMon
        // controller already did).
        #[cfg(windows)]
        if !crate::elevation::is_elevated() {
            return;
        }

        let mut bin_missing_logged = false;
        let mut seen: u64 = 0;
        let mut last_unexpected_exit: Option<Instant> = None;

        loop {
            // Park until the overlay config or the running game changes; only
            // poll periodically while the sidecar is actually up (to reap it).
            let running = child_lock().is_some();
            let want_now = crate::metrics::want_cpu_temp() && crate::metrics::has_game();
            crate::metrics::wait_sidecar(
                &mut seen,
                (want_now || running).then(|| Duration::from_millis(500)),
            );

            let want = crate::metrics::want_cpu_temp() && crate::metrics::has_game();

            if want && child_lock().is_none() && respawn_allowed(last_unexpected_exit, Instant::now()) {
                match find_binary(&app) {
                    Some(bin) => match spawn(&bin) {
                        Ok(c) => *child_lock() = Some(c),
                        Err(e) => {
                            eprintln!("cputemp no pudo iniciarse: {e}");
                            last_unexpected_exit = Some(Instant::now());
                        }
                    },
                    None => {
                        if !bin_missing_logged {
                            eprintln!("cputemp.exe no encontrado: temp. de CPU deshabilitada.");
                            bin_missing_logged = true;
                        }
                    }
                }
            } else if !want {
                // Take the child out before stopping it: `stop` waits up to
                // GRACEFUL_STOP and must not hold the lock while it does.
                let child = child_lock().take();
                if let Some(child) = child {
                    stop(child);
                }
                reset();
                // A fresh request (next game / setting re-enabled) gets a try at once.
                last_unexpected_exit = None;
            }

            // Reap a sidecar that exited on its own (driver blocked, no admin, …).
            let exited = matches!(
                child_lock().as_mut().map(|c| c.try_wait()),
                Some(Ok(Some(_)))
            );
            if exited {
                *child_lock() = None;
                reset();
                if want {
                    last_unexpected_exit = Some(Instant::now());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_lines_parse_to_plausible_temperatures() {
        assert_eq!(parse_temp("54\r"), Some(54));
        assert_eq!(parse_temp(" 199 "), Some(199));
        assert_eq!(parse_temp("0"), None);
        assert_eq!(parse_temp("200"), None);
        assert_eq!(parse_temp("-3"), None);
        assert_eq!(parse_temp("cputemp: open failed"), None);
    }

    #[test]
    fn a_sidecar_that_keeps_dying_is_not_respawned_every_poll() {
        // Regression (MT9): a blocked driver made the sidecar exit at once and the
        // controller relaunched it on every 500 ms poll.
        let t0 = Instant::now();
        assert!(respawn_allowed(None, t0));
        assert!(!respawn_allowed(Some(t0), t0 + Duration::from_millis(500)));
        assert!(!respawn_allowed(Some(t0), t0 + RESPAWN_BACKOFF - Duration::from_millis(1)));
        assert!(respawn_allowed(Some(t0), t0 + RESPAWN_BACKOFF));
    }

    #[test]
    fn a_reading_that_stops_arriving_expires() {
        // Regression (MT8): the last temperature stayed on the HUD after the sidecar
        // stopped printing.
        let stamp = 10_000;
        assert!(crate::metrics::is_fresh(stamp, stamp + MAX_AGE_MS, MAX_AGE_MS));
        assert!(!crate::metrics::is_fresh(stamp, stamp + MAX_AGE_MS + 1, MAX_AGE_MS));
    }
}
