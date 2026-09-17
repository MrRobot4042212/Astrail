//! In-game metrics overlay sampler.
//!
//! A single background thread samples hardware telemetry and draws it straight into
//! the native HUD window via the `overlay` facade (DirectComposition when available,
//! GDI fallback). It is fully idle-cheap: it only samples + draws while the overlay is
//! enabled, a game is running, AND that game is the foreground window (the playtime
//! watcher publishes the current game + pid here). NVIDIA GPUs are read via NVML, AMD
//! GPUs via the LibreHardwareMonitor sidecar (`cputemp`, ADL); on anything else the GPU
//! fields are simply omitted.
//!
//! FPS / frametime come from the sidecar (AMD, no admin, exclusive fullscreen only;
//! borderless games report nothing) or the PresentMon ETW controller (admin only) —
//! both degrade silently to `None`.
//! Everything except CPU temperature and PresentMon works without admin.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
#[cfg(windows)]
use tauri::Emitter;
use tauri::AppHandle;

#[cfg(windows)]
use crate::overlay;

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;

/// Master switch, mirrors `AppSettings.overlay.enabled`.
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);
/// Set while an update is being installed: both sidecar controllers stand down so
/// the shutdown done before the installer runs is not undone by a respawn.
static SIDECARS_SUSPENDED: AtomicBool = AtomicBool::new(false);
/// Whether any FPS-class metric (fps/frametime) is enabled, so PresentMon only
/// runs when its output is actually shown.
static FPS_WANTED: AtomicBool = AtomicBool::new(false);
/// Whether CPU temperature is enabled, so the LHM sidecar (kernel driver) only
/// runs when its output is actually shown.
static CPU_TEMP_WANTED: AtomicBool = AtomicBool::new(false);
/// Whether any GPU metric (usage, temperature, VRAM) is enabled, so the sidecar only
/// reads an AMD GPU when its output is actually shown.
static GPU_WANTED: AtomicBool = AtomicBool::new(false);
/// GPU selector the sidecar reads for the running game ("auto" or an AMD PnP id
/// fragment), published by the sampler when the sidecar is the GPU source. None =
/// NVML or no GPU. See `sidecar_gpu`.
static SIDECAR_GPU: Mutex<Option<String>> = Mutex::new(None);
/// Whether the sampler has picked the GPU source for the running game, i.e. whether
/// `SIDECAR_GPU` means anything yet. Until its first drawn tick it has not, and the
/// sidecar waits: a CPU-only sidecar started at launch was restarted with `--gpu` a
/// moment later, loading its kernel driver twice.
static GPU_ROUTE_KNOWN: AtomicBool = AtomicBool::new(false);
/// Whether the sidecar is currently supplying FPS (AMD's native, admin-free counter).
/// When true the PresentMon controller stays idle — running an ETW session per frame
/// for a number we'd only discard is pure overhead (and needs admin).
static SIDECAR_FPS_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the sampler has decided the FPS source for the running game. Until the
/// first sample of a session it has not, and PresentMon waits: on an AMD machine
/// it used to start an ETW session and tear it down one tick later.
static FPS_SOURCE_KNOWN: AtomicBool = AtomicBool::new(false);
/// How long the sidecar keeps the FPS row to itself without a reading. At the start
/// of a session it covers the sidecar's start (~1 s) and first lines. Past this with
/// no reading (borderless game, driver without the counter, or a game that left
/// exclusive fullscreen), PresentMon takes over until the readings come back.
const SIDECAR_FPS_GRACE: Duration = Duration::from_secs(5);
/// Sampling interval in milliseconds.
static INTERVAL_MS: AtomicU64 = AtomicU64::new(1000);
/// PID of the running game's main process (for PresentMon). 0 = none.
static CURRENT_PID: AtomicU32 = AtomicU32::new(0);
/// Name of the running game the overlay should label, set by the playtime watcher.
static CURRENT_GAME: Mutex<Option<String>> = Mutex::new(None);
/// Which GPU to sample: "auto" | "nvml:<i>" | "pci:<AMD PnP id fragment>". None = "auto".
static GPU_SELECT: Mutex<Option<String>> = Mutex::new(None);
/// Whether the in-game overlay *settings* screen (WebView2 window) is open. While
/// it is, the native HUD hides so the two overlays don't fight for the z-order.
static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);
/// Full overlay render config (colors, font size, which metrics, position…),
/// snapshotted so the native HUD renderer can read it each tick.
static RENDER_CFG: Mutex<Option<crate::models::OverlaySettings>> = Mutex::new(None);
/// Whether a game is currently published (mirrors `CURRENT_GAME.is_some()` as an
/// atomic, so the sidecar controllers do not take a mutex twice a second).
static HAS_GAME: AtomicBool = AtomicBool::new(false);
/// Bumped whenever any live config changes (`configure`, `set_gpu`,
/// `set_render_cfg`, `set_settings_open`). The sampler clones the config only
/// when this moves, instead of cloning ~6 Strings on every tick.
static CFG_GEN: AtomicU64 = AtomicU64::new(0);
/// Auto-reset event the sampler waits on, as a raw HANDLE (0 = not created yet).
/// Lets the thread block indefinitely while the overlay is off or no game is
/// running, and wake instantly when that changes — instead of ticking ~86 000
/// times a day to discover there is nothing to draw.
static WAKE_EVENT: AtomicIsize = AtomicIsize::new(0);

/// Live overlay health, derived from the swapchain's real composition mode:
/// 0 = unknown (no game / not yet measured), 1 = free (HUD on a hardware MPO plane →
/// the game keeps independent-flip), 2 = costing (DWM is compositing the HUD → the game
/// loses independent-flip → FPS/latency hit). Read by the UI to show overlay health.
static OVERLAY_HEALTH: AtomicU8 = AtomicU8::new(0);

/// Live overlay health (see `OVERLAY_HEALTH`): 0 unknown, 1 free, 2 costing.
pub fn overlay_health() -> u8 {
    OVERLAY_HEALTH.load(Ordering::Relaxed)
}

/// Seconds to let the HUD present before trusting its composition mode (the swapchain
/// needs a few presents before `GetFrameStatisticsMedia` returns a stable result).
#[cfg(windows)]
const MEASURE_SECS: u64 = 4;
/// Consecutive COMPOSED readings before classifying the overlay as costing — hysteresis
/// so a transient composed frame doesn't flap the state (or hide the HUD).
#[cfg(windows)]
const COMPOSED_CONFIRM: u8 = 2;

/// Publish a new overlay health value (0/1/2) to the UI, only on change.
#[cfg(windows)]
fn set_health(app: &AppHandle, published: &mut u8, health: u8) {
    if *published != health {
        *published = health;
        OVERLAY_HEALTH.store(health, Ordering::Relaxed);
        let _ = app.emit("overlay-health", health);
    }
}

/// One telemetry sample sent to the overlay window.
#[derive(Clone, Serialize)]
pub struct MetricsSample {
    pub game: Option<String>,
    /// `None` until the meter has a real interval to report (see `CpuMeter`).
    pub cpu_usage: Option<f32>,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpu_usage: Option<u32>,
    pub gpu_temp_c: Option<u32>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub gpu_clock_mhz: Option<u32>,
    pub gpu_power_w: Option<f32>,
    // CPU temperature from the LibreHardwareMonitor sidecar (admin + driver).
    pub cpu_temp_c: Option<u32>,
    // PresentMon (per-process, measured) or the sidecar (fullscreen app, FPS only).
    pub fps: Option<f32>,
    pub frametime_ms: Option<f32>,
}

/// Apply overlay settings live (called on startup, on settings change, on hotkey).
pub fn configure(overlay: &crate::models::OverlaySettings) {
    OVERLAY_ENABLED.store(overlay.enabled, Ordering::Relaxed);
    FPS_WANTED.store(overlay.show_fps || overlay.show_frametime, Ordering::Relaxed);
    GPU_WANTED.store(
        overlay.show_gpu || overlay.show_gpu_temp || overlay.show_vram,
        Ordering::Relaxed,
    );
    CPU_TEMP_WANTED.store(overlay.show_cpu_temp, Ordering::Relaxed);
    INTERVAL_MS.store(overlay.interval_ms.clamp(200, 5000), Ordering::Relaxed);
    bump_config();
}

/// Mark the live config as changed and wake the sampler so it applies it now.
fn bump_config() {
    CFG_GEN.fetch_add(1, Ordering::Relaxed);
    wake();
    wake_sidecars();
}

/// Wake signal for the PresentMon / cputemp controller threads.
///
/// They used to poll twice a second forever — `cputemp` even without the
/// elevation early-out PresentMon had, so a non-admin user paid 2 wakeups a
/// second for a sidecar that could never start.
static SIDECAR_WAKE: (Mutex<u64>, std::sync::Condvar) = (Mutex::new(0), std::sync::Condvar::new());

/// Wake both sidecar controllers.
pub fn wake_sidecars() {
    let (lock, cv) = &SIDECAR_WAKE;
    let mut gen = lock.lock().unwrap_or_else(|e| e.into_inner());
    *gen = gen.wrapping_add(1);
    cv.notify_all();
}

/// Park a sidecar controller until something changes (or `timeout` elapses).
/// `seen` carries the last observed generation, so a wake between two waits is
/// never missed.
pub fn wait_sidecar(seen: &mut u64, timeout: Option<Duration>) {
    let (lock, cv) = &SIDECAR_WAKE;
    let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    let guard = match timeout {
        Some(d) => cv
            .wait_timeout_while(guard, d, |gen| *gen == *seen)
            .map(|(g, _)| g)
            .unwrap_or_else(|e| e.into_inner().0),
        None => cv
            .wait_while(guard, |gen| *gen == *seen)
            .unwrap_or_else(|e| e.into_inner()),
    };
    *seen = *guard;
}

/// Wake the sampler thread (config changed, game started/stopped…).
pub fn wake() {
    #[cfg(windows)]
    {
        let raw = WAKE_EVENT.load(Ordering::Relaxed);
        if raw != 0 {
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::Threading::SetEvent;
            // SAFETY: the handle is created once by the sampler thread and lives
            // for the process lifetime; SetEvent on an auto-reset event is safe
            // to call from any thread.
            unsafe {
                let _ = SetEvent(HANDLE(raw as *mut core::ffi::c_void));
            }
        }
    }
}

/// Set which GPU the sampler reads: "auto" | "nvml:<i>" | "pci:<AMD PnP id fragment>".
pub fn set_gpu(sel: String) {
    *GPU_SELECT.lock().unwrap_or_else(|e| e.into_inner()) = Some(sel);
    bump_config();
}

/// Mark the in-game overlay settings screen open/closed (hides/shows the native HUD).
pub fn set_settings_open(open: bool) {
    SETTINGS_OPEN.store(open, Ordering::Relaxed);
    wake();
}

/// Whether the in-game overlay settings screen is currently open.
pub fn settings_open() -> bool {
    SETTINGS_OPEN.load(Ordering::Relaxed)
}

/// Snapshot the full overlay config for the native HUD renderer.
pub fn set_render_cfg(cfg: crate::models::OverlaySettings) {
    *RENDER_CFG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg);
    bump_config();
}

fn render_cfg() -> Option<crate::models::OverlaySettings> {
    RENDER_CFG.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn current_gpu() -> String {
    GPU_SELECT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| "auto".to_string())
}

/// Published by the playtime watcher each poll: the foreground game (if any).
pub fn set_current_game(name: Option<String>, pid: Option<u32>) {
    CURRENT_PID.store(pid.unwrap_or(0), Ordering::Relaxed);
    let had = HAS_GAME.load(Ordering::Relaxed);
    let has = name.is_some();
    HAS_GAME.store(has, Ordering::Relaxed);
    *CURRENT_GAME.lock().unwrap_or_else(|e| e.into_inner()) = name;
    // Only nudge the threads on a transition: this is called on every watcher
    // poll while a game runs.
    if had != has {
        wake();
        wake_sidecars();
    }
}

fn current_game() -> Option<String> {
    CURRENT_GAME.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// PID of the running game (0 = none). Read by the PresentMon controller.
pub fn current_pid() -> u32 {
    CURRENT_PID.load(Ordering::Relaxed)
}

/// Whether PresentMon should be running: overlay on, an FPS metric enabled, and
/// the sidecar isn't already providing FPS (on AMD it is → no need for the ETW session).
pub fn want_fps() -> bool {
    sidecar_wanted(
        OVERLAY_ENABLED.load(Ordering::Relaxed),
        FPS_WANTED.load(Ordering::Relaxed)
            && FPS_SOURCE_KNOWN.load(Ordering::Relaxed)
            && !SIDECAR_FPS_ACTIVE.load(Ordering::Relaxed),
        SIDECARS_SUSPENDED.load(Ordering::Relaxed),
    )
}

/// Whether the sidecar supplies FPS this tick: it is the GPU source and its last FPS
/// reading (or, before the first one, the session start) is within the grace window.
/// Without the grace, a first tick with no reading yet would hand FPS to PresentMon
/// and take it back on the next one; timing from the last reading rather than the
/// first lets PresentMon take over when AMD's counter stops mid-session.
fn sidecar_owns_fps(
    sidecar_sampled: bool,
    since_last_fps: Option<Duration>,
    session_age: Duration,
) -> bool {
    sidecar_sampled && since_last_fps.unwrap_or(session_age) < SIDECAR_FPS_GRACE
}

/// Publish the FPS source decision, waking the PresentMon controller only when it
/// changed. Both atomics are written once per tick, never cleared and re-set inside
/// one, so the controller cannot observe a transient "PresentMon wanted".
fn publish_fps_source(known: bool, sidecar: bool) {
    let was_sidecar = SIDECAR_FPS_ACTIVE.swap(sidecar, Ordering::Relaxed);
    let was_known = FPS_SOURCE_KNOWN.swap(known, Ordering::Relaxed);
    if was_sidecar != sidecar || was_known != known {
        wake_sidecars();
    }
}

/// Where the sampler reads GPU telemetry from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GpuRoute<'a> {
    /// NVML device index.
    Nvml(u32),
    /// The sidecar, with its `--gpu` selector ("auto" or an AMD PnP id fragment).
    Sidecar(&'a str),
    None,
}

/// Resolve the saved GPU choice against the backends present on this machine.
///
/// A choice that no longer applies (card removed, driver gone, or a legacy `adlx:<i>`
/// value) falls back to "auto": NVIDIA first, then AMD. An AMD choice must be one of
/// `amd_keys` (`system::amd_gpu_keys`, the cards present), which also keeps anything
/// else in a hand-edited settings file off the sidecar's command line.
fn gpu_route<'a>(sel: &'a str, nvml_present: bool, amd_keys: &[String]) -> GpuRoute<'a> {
    if nvml_present {
        if let Some(index) = sel.strip_prefix("nvml:").and_then(|i| i.parse().ok()) {
            return GpuRoute::Nvml(index);
        }
    }
    if amd_keys.iter().any(|key| key == sel) {
        if let Some(fragment) = sel.strip_prefix("pci:") {
            return GpuRoute::Sidecar(fragment);
        }
    }
    if nvml_present {
        GpuRoute::Nvml(0)
    } else if !amd_keys.is_empty() {
        GpuRoute::Sidecar("auto")
    } else {
        GpuRoute::None
    }
}

/// Publish the GPU source decision for the sidecar: whether it is made (`known`) and
/// the selector (None = the sidecar is not the GPU source). Wakes its controller only
/// when something changed: this runs on every drawn tick. The selector is written
/// before `known`, so a controller that sees `known` also sees this game's selector.
fn publish_sidecar_gpu(known: bool, selector: Option<&str>) {
    let mut current = SIDECAR_GPU.lock().unwrap_or_else(|e| e.into_inner());
    let changed = current.as_deref() != selector;
    if changed {
        *current = selector.map(str::to_owned);
    }
    drop(current);
    let was_known = GPU_ROUTE_KNOWN.swap(known, Ordering::Relaxed);
    if changed || was_known != known {
        wake_sidecars();
    }
}

/// Whether the overlay could need the sidecar as its GPU source: overlay on and a GPU
/// metric or FPS shown.
fn sidecar_gpu_wanted() -> bool {
    sidecar_wanted(
        OVERLAY_ENABLED.load(Ordering::Relaxed),
        GPU_WANTED.load(Ordering::Relaxed) || FPS_WANTED.load(Ordering::Relaxed),
        SIDECARS_SUSPENDED.load(Ordering::Relaxed),
    )
}

/// The `--gpu` selector the sidecar should run with, or None when it should not read
/// a GPU: overlay off, no GPU metric or FPS shown, no game, or NVML is the source.
pub fn sidecar_gpu() -> Option<String> {
    if !(sidecar_gpu_wanted() && has_game()) {
        return None;
    }
    SIDECAR_GPU.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether a game is running that may need the sidecar as its GPU source, but the
/// sampler has not picked the source yet (see `GPU_ROUTE_KNOWN`).
pub fn sidecar_gpu_pending() -> bool {
    sidecar_gpu_wanted() && has_game() && !GPU_ROUTE_KNOWN.load(Ordering::Relaxed)
}

/// Whether the CPU-temp sidecar should run: overlay on and CPU temp enabled.
pub fn want_cpu_temp() -> bool {
    sidecar_wanted(
        OVERLAY_ENABLED.load(Ordering::Relaxed),
        CPU_TEMP_WANTED.load(Ordering::Relaxed),
        SIDECARS_SUSPENDED.load(Ordering::Relaxed),
    )
}

/// The gate both sidecar controllers share. A suspension overrides every setting.
fn sidecar_wanted(overlay_enabled: bool, metric_wanted: bool, suspended: bool) -> bool {
    overlay_enabled && metric_wanted && !suspended
}

/// Suspend (or resume) both sidecars and wake their controllers so they act on it.
pub fn set_sidecars_suspended(suspended: bool) {
    SIDECARS_SUSPENDED.store(suspended, Ordering::Relaxed);
    wake_sidecars();
}

/// Whether a game is currently running (the overlay's gate for the sidecar).
pub fn has_game() -> bool {
    HAS_GAME.load(Ordering::Relaxed)
}

/// Process-local monotonic clock for sidecar readings, in milliseconds. Starts at 1
/// so a stored stamp of 0 can mean "never written".
pub fn clock_ms() -> u64 {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Whether a reading stamped at `stamp_ms` (0 = never) is still recent enough to show.
///
/// The sidecar readers only write when a new value arrives, so without this a game
/// that stops presenting (loading screen, hang, minimized) or a sidecar that stops
/// printing kept its last number on the HUD as if it were live.
pub fn is_fresh(stamp_ms: u64, now_ms: u64, max_age_ms: u64) -> bool {
    stamp_ms != 0 && now_ms.saturating_sub(stamp_ms) <= max_age_ms
}

/// Oldest FPS/frametime reading still shown: two sampler ticks, never under 2 s.
pub fn fps_max_age_ms() -> u64 {
    (INTERVAL_MS.load(Ordering::Relaxed) * 2).max(2000)
}

const MB: u64 = 1024 * 1024;

/// How long the sampler stays idle before releasing the GPU telemetry backends
/// and the HUD's DirectComposition stack.
#[cfg(windows)]
const BACKEND_IDLE_SECS: u64 = 60;

/// One initialization attempt per backend release cycle.
///
/// A failed `Nvml::init()` used to be retried on every drawn tick: on an AMD-only
/// machine that was a `LoadLibrary("nvml.dll")` probe per frame of the HUD. The
/// attempt (and the AMD adapter probe) is remembered until the backends are released
/// after `BACKEND_IDLE_SECS` idle, so a driver installed or restarted mid-session is
/// picked up on the next game.
#[derive(Debug, Default)]
struct InitOnce {
    tried: bool,
}

impl InitOnce {
    /// `true` only for the first call since construction or the last `reset`.
    fn should_try(&mut self) -> bool {
        !std::mem::replace(&mut self.tried, true)
    }

    fn reset(&mut self) {
        self.tried = false;
    }
}

/// Block until the wake event fires, a window message arrives, or the timeout
/// elapses; then drain the HUD window's message queue.
///
/// `MsgWaitForMultipleObjectsEx` (rather than `sleep` + `PeekMessage`) is what
/// lets the idle case wait *indefinitely* without leaving the HUD window's queue
/// unattended — an unpumped window makes Windows treat the process as hung.
#[cfg(windows)]
fn wait_tick(timeout_ms: u32) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::UI::WindowsAndMessaging::{
        MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_ALLINPUT,
    };
    let raw = WAKE_EVENT.load(Ordering::Relaxed);
    // SAFETY: `raw` is either 0 or the process-lifetime event handle below.
    unsafe {
        if raw != 0 {
            let handles = [HANDLE(raw as *mut core::ffi::c_void)];
            MsgWaitForMultipleObjectsEx(
                Some(&handles),
                timeout_ms,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
        } else {
            // No wake event (CreateEventW failed): never wait forever on window
            // messages alone, or the HUD would never draw again. Fall back to a
            // 1 s poll, which is what the sampler used to do unconditionally.
            let capped = timeout_ms.min(1000);
            MsgWaitForMultipleObjectsEx(None, capped, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    overlay::pump();
}

/// Virtual-desktop rectangle and DPI scale of a monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorGeometry {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

/// Rectangle and DPI scale of the monitor showing `hwnd` (falls back to the primary).
///
/// Read straight from Win32 on this thread. It used to go through
/// `app.get_webview_window("main").primary_monitor()`, which blocks on a round
/// trip to the main event loop **on every drawn frame** (debt C6) — and always
/// answered with the primary monitor, so the HUD was mispositioned on a
/// secondary display. The origin matters as much as the size: without
/// `rcMonitor.left/top` the corner math placed the HUD on the primary monitor while
/// sizing it for the game's.
#[cfg(windows)]
pub fn monitor_geometry(hwnd: isize) -> MonitorGeometry {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    // SAFETY: plain Win32 queries; `info` is fully initialized with its cbSize.
    unsafe {
        let monitor = MonitorFromWindow(
            HWND(hwnd as *mut core::ffi::c_void),
            MONITOR_DEFAULTTOPRIMARY,
        );
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return MonitorGeometry { left: 0, top: 0, width: 1920, height: 1080, scale: 1.0 };
        }
        let r = info.rcMonitor;
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        MonitorGeometry {
            left: r.left,
            top: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
            scale: dpi_x as f64 / 96.0,
        }
    }
}

/// Put a sidecar FPS reading on the sample.
///
/// The sidecar reports an **integer** FPS for the fullscreen application, so
/// `1000 / fps` is not a frametime measurement: at 143 fps it cannot tell 6.9 ms from
/// 7.0 ms and it hides every stutter inside the second. Showing it as `Frame x.x ms`
/// next to PresentMon-grade values presented a derived number as a measured one, so
/// the frametime row is left empty on this path.
fn apply_sidecar_fps(sample: &mut MetricsSample, fps: f32) {
    sample.fps = Some(fps);
    sample.frametime_ms = None;
}

/// Start the sampler thread. Spawned once from `setup`.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // The wake event this thread parks on. Created here so the handle belongs
        // to the sampler for the whole process lifetime.
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::CreateEventW;
            // SAFETY: auto-reset, unnamed, initially unsignaled event.
            if let Ok(handle) = unsafe { CreateEventW(None, false, false, None) } {
                WAKE_EVENT.store(handle.0 as isize, Ordering::Relaxed);
            }
        }
        // GPU telemetry backends are created on the first tick that actually
        // draws and released after `BACKEND_IDLE_SECS` without a game: loading
        // nvml.dll at startup cost every user memory (and a driver DLL) even with
        // the overlay switched off. AMD GPUs are read by the sidecar; here we only
        // remember which ones there are.
        let mut nvml: Option<Nvml> = None;
        let mut nvml_init = InitOnce::default();
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut amd_keys: Vec<String> = Vec::new();
        #[cfg(windows)]
        let mut amd_init = InitOnce::default();
        #[cfg(windows)]
        let mut backends_idle_since: Option<Instant> = None;
        // Global CPU%/RAM: direct Win32 (GetSystemTimes / GlobalMemoryStatusEx),
        // one syscall each and zero allocation.
        #[cfg(windows)]
        let mut cpu_meter = crate::sysstat::CpuMeter::new();
        // Tracks whether the overlay window is currently shown, to avoid spamming
        // show()/hide() every tick.
        let mut shown = false;
        // Last foreground window we re-asserted topmost against. We only restack
        // when the foreground actually changes (see the topmost block below) —
        // toggling NOTOPMOST→TOPMOST every tick forces DWM to recomposite and
        // knocks the game out of independent-flip, causing a periodic hitch.
        #[cfg(windows)]
        let mut last_fg: isize = 0;
        // Adaptive MPO state (per draw "session" = while one foreground window stays up).
        // We let the HUD present for a short window, read the swapchain's real
        // composition mode, and classify the overlay as free (on a hardware MPO plane)
        // or costing (DWM compositing → the game loses independent-flip). In
        // "performance" mode a stable *costing* result hides the HUD so it never
        // silently drops the game's FPS.
        #[cfg(windows)]
        let mut measure_deadline: Option<std::time::Instant> = None;
        #[cfg(windows)]
        let mut composed_streak: u8 = 0;
        #[cfg(windows)]
        let mut measured = false;
        #[cfg(windows)]
        let mut perf_hidden = false;
        #[cfg(windows)]
        let mut published_health: u8 = 0;
        // Deep diagnostics (opt-in via METEOR_OVERLAY_DEBUG). Tracks the last gating
        // decision so we only log on change, plus a heartbeat timer.
        #[cfg(windows)]
        crate::overlay_diag::init(&app);
        #[cfg(windows)]
        let mut diag_state: (bool, bool, bool) = (false, false, false);
        #[cfg(windows)]
        let mut diag_heartbeat = std::time::Instant::now();
        // Cached live config, refreshed only when `CFG_GEN` moves.
        let mut cfg_gen: u64 = u64::MAX;
        let mut cfg: Option<crate::models::OverlaySettings> = None;
        let mut sel = String::from("auto");
        // FPS source state for the running game (see `sidecar_owns_fps`).
        let mut fps_session_start: Option<Instant> = None;
        let mut sidecar_fps_at: Option<Instant> = None;

        loop {
            // Idle (overlay off, no game, or the settings screen open) → wait with
            // no timeout; a config change or a game starting wakes us instantly.
            let active_now = OVERLAY_ENABLED.load(Ordering::Relaxed)
                && HAS_GAME.load(Ordering::Relaxed)
                && !SETTINGS_OPEN.load(Ordering::Relaxed);
            #[cfg(windows)]
            wait_tick(if active_now {
                INTERVAL_MS.load(Ordering::Relaxed) as u32
            } else {
                windows::Win32::System::Threading::INFINITE
            });
            #[cfg(not(windows))]
            std::thread::sleep(Duration::from_millis(INTERVAL_MS.load(Ordering::Relaxed)));

            // Refresh the cached config only when something actually changed.
            let gen = CFG_GEN.load(Ordering::Relaxed);
            if gen != cfg_gen {
                cfg_gen = gen;
                cfg = render_cfg();
                sel = current_gpu();
            }

            // Idle path: overlay off, no game, or the in-game settings screen open →
            // keep the native HUD hidden (the WebView2 window shows the settings).
            let active = OVERLAY_ENABLED.load(Ordering::Relaxed);
            let settings_open = SETTINGS_OPEN.load(Ordering::Relaxed);
            let raw_game = if active && !settings_open { current_game() } else { None };
            // Only draw while the game is the *foreground* window. If you alt-tab out,
            // the game stays "running" (so playtime keeps counting) but drawing a
            // topmost HUD over the desktop would force composition for nothing — and we
            // skip sampling entirely too. If the pid is unknown (0) we don't gate, to
            // avoid hiding the HUD on a process we couldn't resolve.
            #[cfg(windows)]
            let pid = CURRENT_PID.load(Ordering::Relaxed);
            #[cfg(windows)]
            let fg_pid = if raw_game.is_some() { overlay::foreground_pid() } else { 0 };
            // Only draw while the game is the foreground window. Alt-tabbed out, drawing
            // a topmost HUD over the desktop forces composition for nothing — and on an
            // MPO-denied config that costs the same as in-game. If the pid is unknown (0)
            // we don't gate, to avoid hiding the HUD on a process we couldn't resolve.
            #[cfg(windows)]
            let game = if raw_game.is_some() && pid != 0 && fg_pid != pid {
                None
            } else {
                raw_game.clone()
            };
            #[cfg(not(windows))]
            let game = raw_game;
            // Diagnostics: log the gating decision whenever it changes, with the
            // foreground window classified (the condition that decides MPO).
            #[cfg(windows)]
            if crate::overlay_diag::enabled() {
                let state = (active, raw_game.is_some(), game.is_some());
                if state != diag_state {
                    crate::overlay_diag::log(&format!(
                        "gate: overlay_on={active} settings_open={settings_open} game={:?} pid={pid} fg_pid={fg_pid} → dibujar={} | {}",
                        raw_game,
                        game.is_some(),
                        crate::overlay_diag::foreground_report()
                    ));
                    diag_state = state;
                }
            }
            if game.is_none() {
                // The game is gone (not just alt-tabbed): decide the FPS and GPU
                // sources afresh for the next one.
                if !HAS_GAME.load(Ordering::Relaxed) {
                    fps_session_start = None;
                    sidecar_fps_at = None;
                    publish_fps_source(false, false);
                    publish_sidecar_gpu(false, None);
                }
                // Re-prime on the way back so the first CPU% after a pause is not
                // averaged over the whole time the sampler was parked.
                #[cfg(windows)]
                cpu_meter.reset();
                if shown {
                    #[cfg(windows)]
                    overlay::hide();
                    shown = false;
                    #[cfg(windows)]
                    {
                        last_fg = 0; // re-assert topmost when we show again
                    }
                }
                // Release the GPU backends and the HUD's DirectComposition stack
                // after a while with no game. `hide()` only hides the window: the
                // D3D11 device, swapchain, D2D context and HWND used to stay
                // resident for the rest of the session once a game had run.
                #[cfg(windows)]
                {
                    let idle_for = backends_idle_since.get_or_insert_with(Instant::now);
                    if idle_for.elapsed() >= Duration::from_secs(BACKEND_IDLE_SECS) {
                        nvml = None;
                        amd_keys = Vec::new();
                        nvml_init.reset();
                        amd_init.reset();
                        overlay::teardown();
                        backends_idle_since = None; // released; nothing left to do
                    }
                }
                // No game in the foreground → health is meaningless; clear it and reset
                // the adaptive measure state so the next session re-measures from scratch.
                #[cfg(windows)]
                {
                    measured = false;
                    perf_hidden = false;
                    composed_streak = 0;
                    if published_health != 0 {
                        published_health = 0;
                        OVERLAY_HEALTH.store(0, Ordering::Relaxed);
                        let _ = app.emit("overlay-health", 0u8);
                    }
                }
                continue;
            }

            // A game is being drawn: make sure the telemetry backends exist.
            #[cfg(windows)]
            {
                backends_idle_since = None;
                if amd_init.should_try() {
                    amd_keys = crate::system::amd_gpu_keys();
                }
            }
            if nvml.is_none() && nvml_init.should_try() {
                // A driver without a device (GPU removed, eGPU unplugged) is no
                // NVIDIA: "auto" must fall through to AMD.
                nvml = Nvml::init()
                    .ok()
                    .filter(|n| n.device_count().is_ok_and(|count| count > 0));
            }

            // CPU + RAM. The first reading after a (re)prime is `None` and the HUD
            // omits the row for that tick rather than drawing a made-up 0 %.
            #[cfg(windows)]
            let (cpu_usage, ram_used_mb, ram_total_mb) = {
                let (used, total) = crate::sysstat::mem_mb();
                (cpu_meter.pct(), used, total)
            };
            // Non-Windows builds have no telemetry source (the whole overlay is
            // Win32); the sampler still runs so the module compiles and tests.
            #[cfg(not(windows))]
            let (cpu_usage, ram_used_mb, ram_total_mb) = (None::<f32>, 0u64, 0u64);

            let mut sample = MetricsSample {
                game,
                cpu_usage,
                ram_used_mb,
                ram_total_mb,
                gpu_usage: None,
                gpu_temp_c: None,
                vram_used_mb: None,
                vram_total_mb: None,
                gpu_clock_mhz: None,
                gpu_power_w: None,
                cpu_temp_c: None,
                fps: None,
                frametime_ms: None,
            };
            // The LibreHardwareMonitor sidecar's latest reading. CPU temperature is
            // None unless it runs with admin + a loadable driver.
            let reading = crate::cputemp::current();
            sample.cpu_temp_c = reading.and_then(|r| r.cpu_temp_c);

            // FPS / frametime from the PresentMon controller (None unless it's
            // running with admin + the bundled binary). The sidecar path below may
            // override this with AMD's native FPS.
            let (fps, frametime) = crate::presentmon::current();
            sample.fps = fps;
            sample.frametime_ms = frametime;

            // Which GPU to read (see `gpu_route`). `sel` is refreshed at the top of
            // the loop only when the config generation moved.
            let route = gpu_route(&sel, nvml.is_some(), &amd_keys);
            let sidecar_sampled = matches!(route, GpuRoute::Sidecar(_));
            publish_sidecar_gpu(true, match route {
                GpuRoute::Sidecar(selector) => Some(selector),
                _ => None,
            });
            match route {
                // Right after a route change this can still be the previous
                // sidecar's reading, for at most the second it takes to restart.
                GpuRoute::Sidecar(_) => {
                    if let Some(r) = reading {
                        sample.gpu_usage = r.gpu_usage;
                        sample.gpu_temp_c = r.gpu_temp_c;
                        sample.vram_used_mb = r.vram_used_mb;
                        sample.vram_total_mb = r.vram_total_mb;
                        sample.gpu_clock_mhz = r.gpu_clock_mhz;
                        sample.gpu_power_w = r.gpu_power_w;
                        // AMD's FPS of the fullscreen app (no PID targeting, no
                        // admin): preferred over PresentMon when present, and it
                        // keeps the PresentMon controller idle (no ETW session).
                        if let Some(f) = r.fps {
                            apply_sidecar_fps(&mut sample, f);
                            sidecar_fps_at = Some(Instant::now());
                        }
                    }
                }
                GpuRoute::Nvml(index) => {
                    if let Some(dev) = nvml.as_ref().and_then(|n| n.device_by_index(index).ok()) {
                        if let Ok(u) = dev.utilization_rates() {
                            sample.gpu_usage = Some(u.gpu);
                        }
                        if let Ok(t) = dev.temperature(TemperatureSensor::Gpu) {
                            sample.gpu_temp_c = Some(t);
                        }
                        if let Ok(mem) = dev.memory_info() {
                            sample.vram_used_mb = Some(mem.used / MB);
                            sample.vram_total_mb = Some(mem.total / MB);
                        }
                        if let Ok(clk) = dev.clock_info(Clock::Graphics) {
                            sample.gpu_clock_mhz = Some(clk);
                        }
                        if let Ok(mw) = dev.power_usage() {
                            sample.gpu_power_w = Some(mw as f32 / 1000.0);
                        }
                    }
                }
                GpuRoute::None => {}
            }
            let session_age = fps_session_start.get_or_insert_with(Instant::now).elapsed();
            let sidecar_fps = sidecar_owns_fps(
                sidecar_sampled,
                sidecar_fps_at.map(|at| at.elapsed()),
                session_age,
            );
            publish_fps_source(true, sidecar_fps);

            // Draw the native HUD via the overlay facade: a content-sized window backed
            // by a DirectComposition flip swapchain (MPO-friendly → the game keeps its
            // independent-flip, low-latency path), or the GDI layered window as fallback.
            // No Chromium compositor either way. The window is owned + pumped by this
            // thread, and only drawn when its content changed (present-on-change).
            // NOTE: true exclusive-fullscreen (D3D independent flip) bypasses the DWM
            // compositor entirely — no HWND overlay can appear on top without DLL
            // injection into the game process.
            #[cfg(windows)]
            {
                if let Some(cfg) = cfg.as_ref() {
                    // Geometry of the monitor the game is on, read directly from
                    // Win32 on this thread (no round trip to the main event loop).
                    let monitor = monitor_geometry(overlay::foreground());

                    // A foreground change starts a fresh measure "session": let the HUD
                    // present for a moment, then read the real composition mode. Also the
                    // moment to re-assert topmost (the NOTOPMOST→TOPMOST toggle forces a
                    // DWM recomposite, so gate it on a real change to avoid a periodic hitch).
                    let fg = overlay::foreground();
                    if fg != 0 && fg != last_fg {
                        last_fg = fg;
                        measure_deadline =
                            Some(std::time::Instant::now() + Duration::from_secs(MEASURE_SECS));
                        composed_streak = 0;
                        measured = false;
                        perf_hidden = false;
                        overlay::reassert_topmost();
                    }

                    let performance = cfg.mpo_mode == "performance";
                    if perf_hidden {
                        // Stable *costing* + performance mode → keep the HUD hidden so it
                        // never silently drops the game's FPS. Re-measured on the next
                        // foreground change.
                        if shown {
                            overlay::hide();
                            shown = false;
                        }
                    } else {
                        overlay::render(cfg, &sample, monitor);
                        shown = true;

                        // Classify free vs costing once the present window has settled
                        // (the swapchain needs a few presents before its composition mode
                        // is reliable). Done once per session; re-armed on foreground change.
                        let settled = measure_deadline
                            .map(|d| std::time::Instant::now() >= d)
                            .unwrap_or(true);
                        if !measured && settled {
                            match overlay::composition_mode() {
                                Some(1) => {
                                    // OVERLAY: HUD on a hardware plane → free.
                                    composed_streak = 0;
                                    measured = true;
                                    set_health(&app, &mut published_health, 1);
                                }
                                Some(0) => {
                                    // COMPOSED: DWM is compositing the HUD → costing the game.
                                    composed_streak = composed_streak.saturating_add(1);
                                    if composed_streak >= COMPOSED_CONFIRM {
                                        measured = true;
                                        set_health(&app, &mut published_health, 2);
                                        perf_hidden = performance;
                                    }
                                }
                                _ => {} // NONE / FAILURE / not measurable yet → keep trying
                            }
                        }
                    }

                    // Heartbeat (~every 3 s while drawing): the live sample + the
                    // swapchain composition mode (the definitive MPO check, queried
                    // inside the dcomp backend).
                    if crate::overlay_diag::enabled()
                        && diag_heartbeat.elapsed() >= Duration::from_secs(3)
                    {
                        diag_heartbeat = std::time::Instant::now();
                        crate::overlay_diag::log(&format!(
                            "muestra: fps={:?} frame={:?}ms gpu={:?}% gpuTemp={:?}°C cpu={:?}% cpuTemp={:?}°C ram={}/{}MB",
                            sample.fps,
                            sample.frametime_ms,
                            sample.gpu_usage,
                            sample.gpu_temp_c,
                            sample.cpu_usage,
                            sample.cpu_temp_c,
                            sample.ram_used_mb,
                            sample.ram_total_mb
                        ));
                        overlay::log_composition_mode();
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
    fn the_sidecar_keeps_fps_through_its_first_ticks_so_presentmon_is_not_started_and_killed() {
        // Regression (MT7): the first AMD tick had no FPS reading yet, so PresentMon
        // was spawned (ETW session) and torn down one tick later on every launch.
        let early = Duration::from_millis(500);
        assert!(sidecar_owns_fps(true, None, early));
        // No FPS reading (borderless game, driver without the counter): hands over
        // after the grace window.
        assert!(!sidecar_owns_fps(true, None, SIDECAR_FPS_GRACE));
        // NVML sampled (sidecar not the GPU source): always PresentMon.
        assert!(!sidecar_owns_fps(false, None, early));
        assert!(!sidecar_owns_fps(false, Some(Duration::ZERO), early));
    }

    #[test]
    fn presentmon_takes_over_when_the_sidecar_fps_stops_mid_session() {
        // Regression: a game that left exclusive fullscreen stopped AMD's counter, but
        // one earlier reading kept the FPS row on the sidecar, empty, all session.
        let late = Duration::from_secs(600);
        assert!(sidecar_owns_fps(true, Some(Duration::from_secs(1)), late));
        assert!(!sidecar_owns_fps(true, Some(SIDECAR_FPS_GRACE), late));
        // And the sidecar gets it back as soon as its readings return.
        assert!(sidecar_owns_fps(true, Some(Duration::ZERO), late));
    }

    #[test]
    fn the_gpu_choice_falls_back_to_what_this_machine_has() {
        const RX: &str = "pci:VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0";
        const IGPU: &str = "pci:VEN_1002&DEV_13C0&SUBSYS_88771043&REV_C9";
        let rx = &RX["pci:".len()..];
        let both = [RX.to_string(), IGPU.to_string()];
        let only_igpu = [IGPU.to_string()];
        // Auto: NVIDIA first, then AMD.
        assert_eq!(gpu_route("auto", true, &both), GpuRoute::Nvml(0));
        assert_eq!(gpu_route("auto", false, &both), GpuRoute::Sidecar("auto"));
        assert_eq!(gpu_route("auto", false, &[]), GpuRoute::None);
        // An explicit choice wins while its card is there.
        assert_eq!(gpu_route("nvml:1", true, &both), GpuRoute::Nvml(1));
        assert_eq!(gpu_route(RX, true, &both), GpuRoute::Sidecar(rx));
        // Otherwise it behaves as auto.
        assert_eq!(gpu_route("nvml:1", false, &both), GpuRoute::Sidecar("auto"));
        assert_eq!(gpu_route(RX, true, &[]), GpuRoute::Nvml(0));
        // Regression: a saved AMD card that was removed, while another AMD GPU stays,
        // was still passed to the sidecar, which matched nothing.
        assert_eq!(gpu_route(RX, false, &only_igpu), GpuRoute::Sidecar("auto"));
        // Settings saved by the ADLX build.
        assert_eq!(gpu_route("adlx:0", false, &both), GpuRoute::Sidecar("auto"));
    }

    #[test]
    fn only_a_present_amd_card_reaches_the_sidecar_command_line() {
        let keys = ["pci:VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0".to_string()];
        for sel in [
            "pci:VEN_1002\" --cpu",
            "pci:VEN_1002&DEV_7550",
            "pci:VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0 --cpu",
            "VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0",
        ] {
            assert_eq!(gpu_route(sel, false, &keys), GpuRoute::Sidecar("auto"), "{sel}");
        }
    }

    #[test]
    fn a_reading_is_fresh_only_within_its_max_age() {
        // Regression (MT2/MT8): a value that stopped updating stayed on the HUD forever.
        assert!(is_fresh(1_000, 1_000, 2_000));
        assert!(is_fresh(1_000, 3_000, 2_000));
        assert!(!is_fresh(1_000, 3_001, 2_000));
        // 0 means "never written", whatever the clock says.
        assert!(!is_fresh(0, 1, 2_000));
        // A stamp from the future (racing writer) is not stale.
        assert!(is_fresh(5_000, 4_000, 2_000));
    }

    #[test]
    fn fps_max_age_is_two_ticks_with_a_two_second_floor() {
        let prev = INTERVAL_MS.load(Ordering::Relaxed);
        INTERVAL_MS.store(250, Ordering::Relaxed);
        assert_eq!(fps_max_age_ms(), 2_000);
        INTERVAL_MS.store(5_000, Ordering::Relaxed);
        assert_eq!(fps_max_age_ms(), 10_000);
        INTERVAL_MS.store(prev, Ordering::Relaxed);
    }

    fn empty_sample() -> MetricsSample {
        MetricsSample {
            game: None,
            cpu_usage: None,
            ram_used_mb: 0,
            ram_total_mb: 0,
            gpu_usage: None,
            gpu_temp_c: None,
            vram_used_mb: None,
            vram_total_mb: None,
            gpu_clock_mhz: None,
            gpu_power_w: None,
            cpu_temp_c: None,
            fps: None,
            frametime_ms: None,
        }
    }

    #[test]
    fn sidecar_fps_does_not_invent_a_frametime() {
        // Regression (MT5): `1000 / integer fps` was drawn as a measured frametime,
        // and it also overwrote a stale PresentMon frametime from another source.
        let mut s = empty_sample();
        s.frametime_ms = Some(4.2);
        apply_sidecar_fps(&mut s, 143.0);
        assert_eq!(s.fps, Some(143.0));
        assert_eq!(s.frametime_ms, None);
    }

    #[test]
    fn a_suspended_sidecar_is_not_wanted_whatever_the_settings() {
        // Regression (BD1): the update installer exits the process without running
        // `RunEvent::Exit`, so the sidecars are stopped beforehand; the controllers
        // must not bring them back while the download finishes.
        assert!(sidecar_wanted(true, true, false));
        assert!(!sidecar_wanted(true, true, true));
        assert!(!sidecar_wanted(false, true, false));
        assert!(!sidecar_wanted(true, false, false));
    }

    #[test]
    fn a_failed_backend_init_is_not_retried_every_tick() {
        // Regression (W4): a missing nvml.dll was re-probed on every drawn tick.
        let mut init = InitOnce::default();
        assert!(init.should_try());
        for _ in 0..100 {
            assert!(!init.should_try());
        }
        // Released after the idle period → the next game gets one fresh attempt.
        init.reset();
        assert!(init.should_try());
        assert!(!init.should_try());
    }

    #[test]
    fn clock_never_returns_the_never_written_sentinel() {
        assert!(clock_ms() >= 1);
    }
}
