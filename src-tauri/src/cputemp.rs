// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Hardware readings via the LibreHardwareMonitor sidecar (`binaries/cputemp.exe`,
//! built from `sidecar/cputemp/`; the name predates its GPU mode).
//!
//! - **CPU temperature** (`--cpu`): LHM reads Ryzen Tctl / Intel core temps through
//!   a kernel driver, so this needs **admin** and an HVCI-compatible driver. It is
//!   only requested when Astrail is elevated.
//! - **AMD GPU telemetry and FPS** (`--gpu <selector>`): read through ADL, which
//!   ships with AMD's graphics driver. No admin and no kernel driver. The sampler
//!   decides when the sidecar is the GPU source (`metrics::sidecar_gpu`).
//!
//! Whatever the sidecar cannot read is left out of its output and degrades to
//! `None`, the same best-effort contract as the PresentMon (FPS) integration.
//!
//! A controller thread runs the sidecar only while the overlay wants one of its
//! readings and a game is running, restarts it when the wanted mode changes,
//! parses its `key=value` lines and keeps the latest one for the sampler.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// One line of sidecar output. Every field is optional: the sidecar leaves out
/// what it cannot read, and values outside a plausible range are dropped here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    pub cpu_temp_c: Option<u32>,
    pub gpu_usage: Option<u32>,
    pub gpu_temp_c: Option<u32>,
    pub gpu_power_w: Option<f32>,
    pub gpu_clock_mhz: Option<u32>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    /// Frames per second of the fullscreen application, as AMD's driver counts them.
    pub fps: Option<f32>,
}

/// The latest reading and the sidecar it belongs to.
struct Latest {
    /// Bumped on every spawn. A reader thread only writes while its generation is
    /// current, so the old sidecar's EOF cannot wipe the new one's first reading
    /// after a mode change.
    generation: u64,
    /// The reading and its `metrics::clock_ms()` stamp.
    reading: Option<(Reading, u64)>,
}

static LATEST: Mutex<Latest> = Mutex::new(Latest { generation: 0, reading: None });

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

fn latest_lock() -> MutexGuard<'static, Latest> {
    LATEST.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The sidecar's latest reading, if it produced one recently.
pub fn current() -> Option<Reading> {
    let (reading, stamp) = latest_lock().reading?;
    crate::metrics::is_fresh(stamp, crate::metrics::clock_ms(), MAX_AGE_MS).then_some(reading)
}

fn reset() {
    latest_lock().reading = None;
}

/// What the sidecar is asked to read, i.e. its command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Mode {
    cpu: bool,
    /// GPU selector: "auto" or an AMD PnP id fragment (see `metrics::gpu_route`).
    gpu: Option<String>,
}

impl Mode {
    fn is_idle(&self) -> bool {
        !self.cpu && self.gpu.is_none()
    }

    fn args(&self) -> Vec<&str> {
        let mut args = Vec::new();
        if self.cpu {
            args.push("--cpu");
        }
        if let Some(selector) = &self.gpu {
            args.extend(["--gpu", selector.as_str()]);
        }
        args
    }
}

/// The mode the overlay wants right now. CPU temperature is only asked for when
/// elevated: without admin the driver cannot load and the reading never comes.
fn wanted_mode(elevated: bool) -> Mode {
    use crate::metrics::{has_game, sidecar_gpu, sidecar_gpu_pending, want_cpu_temp};
    mode_for(
        elevated && want_cpu_temp() && has_game(),
        sidecar_gpu_pending(),
        sidecar_gpu(),
    )
}

/// The mode for these wants. Nothing starts while the sampler has yet to pick the GPU
/// source: a CPU-only sidecar started then is restarted with `--gpu` on the first
/// drawn tick, loading its kernel driver twice, and a stop that lands while it is
/// still starting can end in a kill that leaves the driver loaded (see `stop`).
fn mode_for(cpu: bool, gpu_pending: bool, gpu: Option<String>) -> Mode {
    if gpu_pending {
        return Mode::default();
    }
    Mode { cpu, gpu }
}

/// Parse one sidecar line (`key=value` pairs), dropping implausible values.
fn parse_line(line: &str) -> Reading {
    fn number<T: std::str::FromStr>(value: &str) -> Option<T> {
        value.parse().ok()
    }
    fn finite(value: &str) -> Option<f32> {
        number::<f32>(value).filter(|v| v.is_finite())
    }
    fn temp(value: &str) -> Option<u32> {
        number::<u32>(value).filter(|v| *v > 0 && *v < 200)
    }

    let mut r = Reading::default();
    for pair in line.split_ascii_whitespace() {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "cpu_temp" => r.cpu_temp_c = temp(value),
            "gpu_usage" => r.gpu_usage = number::<u32>(value).filter(|v| *v <= 100),
            "gpu_temp" => r.gpu_temp_c = temp(value),
            "gpu_power" => r.gpu_power_w = finite(value).filter(|v| (0.0..2000.0).contains(v)),
            "gpu_clock" => r.gpu_clock_mhz = number(value),
            "vram_used" => r.vram_used_mb = number(value),
            "vram_total" => r.vram_total_mb = number::<u64>(value).filter(|v| *v > 0),
            "fps" => r.fps = finite(value).filter(|v| *v > 0.0),
            // A newer sidecar may print keys this build does not know.
            _ => {}
        }
    }
    r
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

/// Spawn the sidecar in `mode` with a reader thread parsing its stdout.
fn spawn(bin: &PathBuf, mode: &Mode) -> std::io::Result<Child> {
    use crate::sidecar_integrity::{open_verified, CPUTEMP_SHA256};
    // Held until the process exists: see `sidecar_integrity`.
    let _pinned = open_verified(bin, CPUTEMP_SHA256)?;
    let mut cmd = Command::new(bin);
    cmd.args(mode.args());
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
    let generation = next_generation();
    // Kill-on-close job: last-resort backstop so an orphaned elevated sidecar cannot
    // outlive Astrail after a crash. Note it terminates rather than stops the child,
    // which does NOT unload the driver — that is what `stop` is for.
    #[cfg(windows)]
    crate::jobobj::assign(&child);
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                // Every line counts, even an empty one: it proves the sidecar is alive
                // and clears values it stopped reporting.
                if !record(generation, Some(parse_line(&line))) {
                    return;
                }
            }
            // Stream ended (sidecar exited): clear the stale reading.
            record(generation, None);
        });
    }
    Ok(child)
}

/// Start a new generation for a sidecar just spawned, dropping the previous reading.
fn next_generation() -> u64 {
    let mut latest = latest_lock();
    latest.generation += 1;
    latest.reading = None;
    latest.generation
}

/// Store what a reader thread saw (a line, or None once its stream ended), unless its
/// sidecar has been replaced since. Returns whether it is still the current one.
fn record(generation: u64, reading: Option<Reading>) -> bool {
    let mut latest = latest_lock();
    if latest.generation != generation {
        return false;
    }
    latest.reading = reading.map(|r| (r, crate::metrics::clock_ms()));
    true
}

/// Stop the sidecar so it unloads its kernel driver, killing it only if it refuses.
///
/// Dropping its stdin is the agreed shutdown signal: the sidecar sees EOF, calls
/// `Close()` (which unloads the LibreHardwareMonitor driver and stops ADL's
/// logging) and exits. `kill()` alone is TerminateProcess, which skips .NET
/// finalizers and leaves the driver loaded and registered for the rest of the boot
/// — a documented local privilege-escalation primitive and a kernel-anti-cheat
/// blocklist trigger.
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

/// Start the controller thread. Idle until the overlay wants a sidecar reading and
/// a game is running; restarts the sidecar when the wanted mode changes and tears
/// it down (unloading its driver) when nothing is wanted.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // Elevation cannot change while we run, so check once.
        #[cfg(windows)]
        let elevated = crate::elevation::is_elevated();
        #[cfg(not(windows))]
        let elevated = false;

        let mut bin_missing_logged = false;
        let mut seen: u64 = 0;
        let mut last_unexpected_exit: Option<Instant> = None;
        // Mode of the running sidecar (None = not running), and the last wanted one.
        let mut running: Option<Mode> = None;
        let mut last_want = Mode::default();

        loop {
            // Park until the overlay config, the running game or the GPU route
            // changes; only poll periodically while the sidecar is wanted or up (to
            // reap it).
            let busy = running.is_some() || !wanted_mode(elevated).is_idle();
            crate::metrics::wait_sidecar(&mut seen, busy.then(|| Duration::from_millis(500)));

            let want = wanted_mode(elevated);
            if want != last_want {
                // A fresh request (next game, setting or GPU changed) gets a try at once.
                last_unexpected_exit = None;
                last_want = want.clone();
            }

            // No longer wanted, or wanted with other arguments: stop it. A changed mode
            // is respawned just below.
            if running.as_ref().is_some_and(|mode| *mode != want) {
                // Take the child out before stopping it: `stop` waits up to
                // GRACEFUL_STOP and must not hold the lock while it does.
                let child = child_lock().take();
                if let Some(child) = child {
                    stop(child);
                }
                running = None;
                reset();
            }

            if !want.is_idle()
                && running.is_none()
                && respawn_allowed(last_unexpected_exit, Instant::now())
            {
                match find_binary(&app) {
                    Some(bin) => match spawn(&bin, &want) {
                        Ok(c) => {
                            *child_lock() = Some(c);
                            running = Some(want);
                        }
                        Err(e) => {
                            eprintln!("cputemp no pudo iniciarse: {e}");
                            last_unexpected_exit = Some(Instant::now());
                        }
                    },
                    None => {
                        if !bin_missing_logged {
                            eprintln!("cputemp.exe not found: CPU temperature and AMD GPU metrics disabled");
                            bin_missing_logged = true;
                        }
                    }
                }
            }

            // Reap a sidecar that exited on its own (driver blocked, crash, …) or that
            // `shutdown` already took.
            if running.is_some() {
                let mut child = child_lock();
                let exited = match child.as_mut() {
                    None => Some(false),
                    Some(c) => matches!(c.try_wait(), Ok(Some(_))).then_some(true),
                };
                if let Some(unexpected) = exited {
                    *child = None;
                    drop(child);
                    running = None;
                    reset();
                    if unexpected {
                        last_unexpected_exit = Some(Instant::now());
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_sidecar_line_parses_every_key() {
        let line = "cpu_temp=54 gpu_usage=9 gpu_temp=44 gpu_power=27.4 gpu_clock=152 \
                    vram_used=4340 vram_total=16304 fps=144\r";
        assert_eq!(
            parse_line(line),
            Reading {
                cpu_temp_c: Some(54),
                gpu_usage: Some(9),
                gpu_temp_c: Some(44),
                gpu_power_w: Some(27.4),
                gpu_clock_mhz: Some(152),
                vram_used_mb: Some(4340),
                vram_total_mb: Some(16304),
                fps: Some(144.0),
            }
        );
    }

    #[test]
    fn every_key_the_sidecar_prints_is_parsed() {
        // Unknown keys are ignored, so a key renamed on one side only would blank its
        // HUD row without any error.
        let source = include_str!("../sidecar/cputemp/Program.cs");
        let printed: Vec<&str> = source
            .split("Put(\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert_eq!(printed.len(), 8, "{printed:?}");
        for key in printed {
            assert_ne!(parse_line(&format!("{key}=1")), Reading::default(), "{key}");
        }
    }

    #[test]
    fn missing_and_implausible_values_are_none() {
        // An empty line (no admin and no AMD GPU) is a valid, empty reading.
        assert_eq!(parse_line(""), Reading::default());
        assert_eq!(parse_line("cputemp: open failed"), Reading::default());
        assert_eq!(parse_line("cpu_temp=0").cpu_temp_c, None);
        assert_eq!(parse_line("cpu_temp=199").cpu_temp_c, Some(199));
        assert_eq!(parse_line("cpu_temp=200").cpu_temp_c, None);
        assert_eq!(parse_line("gpu_temp=-3").gpu_temp_c, None);
        assert_eq!(parse_line("gpu_usage=101").gpu_usage, None);
        assert_eq!(parse_line("gpu_power=NaN").gpu_power_w, None);
        assert_eq!(parse_line("gpu_power=inf").gpu_power_w, None);
        assert_eq!(parse_line("vram_total=0").vram_total_mb, None);
        assert_eq!(parse_line("fps=-1").fps, None);
        // Unknown keys are ignored; the known ones around them still parse.
        assert_eq!(parse_line("gpu_fan=1200 gpu_usage=7").gpu_usage, Some(7));
    }

    #[test]
    fn the_command_line_follows_the_mode() {
        let cpu = Mode { cpu: true, gpu: None };
        let gpu = Mode { cpu: false, gpu: Some("VEN_1002&DEV_7550".into()) };
        let both = Mode { cpu: true, gpu: Some("auto".into()) };
        assert!(Mode::default().is_idle());
        assert!(!cpu.is_idle() && !gpu.is_idle());
        assert_eq!(cpu.args(), ["--cpu"]);
        assert_eq!(gpu.args(), ["--gpu", "VEN_1002&DEV_7550"]);
        assert_eq!(both.args(), ["--cpu", "--gpu", "auto"]);
    }

    #[test]
    fn nothing_starts_until_the_gpu_source_is_known() {
        // Regression: at launch the controller started `--cpu`, then restarted it with
        // `--gpu auto` once the sampler drew its first tick.
        assert!(mode_for(true, true, None).is_idle());
        assert_eq!(mode_for(true, false, None), Mode { cpu: true, gpu: None });
        assert_eq!(
            mode_for(true, false, Some("auto".into())),
            Mode { cpu: true, gpu: Some("auto".into()) }
        );
    }

    #[test]
    fn an_old_sidecar_cannot_overwrite_the_new_ones_reading() {
        // After a mode change the old sidecar's last line, or its EOF, can arrive once
        // the new one is already running.
        let old = next_generation();
        let new = next_generation();
        let reading = Reading { gpu_usage: Some(50), ..Reading::default() };
        assert!(record(new, Some(reading)));
        assert!(!record(old, Some(Reading::default())));
        assert!(!record(old, None));
        assert_eq!(current(), Some(reading));
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
