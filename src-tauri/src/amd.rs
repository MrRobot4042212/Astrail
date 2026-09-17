//! AMD GPU telemetry via the ADLX SDK (C++ shim in `third_party/adlx_shim.cpp`).
//!
//! Counterpart to the NVML (NVIDIA) path in `metrics.rs`: when no NVIDIA GPU is
//! present, the sampler falls back here so AMD Radeon cards still report usage,
//! temperature, VRAM, clocks and power. ADLX loads `amdadlx64.dll` (shipped with
//! the Adrenalin driver) at runtime; on non-AMD systems `init` returns false and
//! the overlay simply omits GPU metrics — the same graceful degradation as NVML.
//!
//! The SDK is not assumed thread-safe: every FFI call goes through `ADLX_LOCK`.
//! The sampler owns the long-lived session; other callers use
//! `list_gpus_transient`.


use std::sync::{Mutex, MutexGuard, PoisonError};

/// Serializes all ADLX FFI calls: the metrics sampler (its own thread) and the
/// `system_info` command (a Tauri command thread) can both reach into ADLX, and
/// the SDK isn't guaranteed thread-safe across concurrent calls.
///
/// The value records whether the **sampler** holds ADLX up (`init` succeeded and
/// `shutdown` has not run). `list_gpus_transient` reads it so a settings screen
/// neither leaves ADLX resident after the sampler released it nor tears it down
/// under a running HUD.
static ADLX_LOCK: Mutex<bool> = Mutex::new(false);

fn lock() -> MutexGuard<'static, bool> {
    // A panic elsewhere must not take GPU telemetry down with it on the sampler.
    ADLX_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

// Validity bits in `AdlxSample.flags`, mirroring the C++ shim.
const F_USAGE: u32 = 1 << 0;
const F_TEMP: u32 = 1 << 1;
const F_HOTSPOT: u32 = 1 << 2;
const F_POWER: u32 = 1 << 3;
const F_CLOCK: u32 = 1 << 4;
const F_VRAM_CLOCK: u32 = 1 << 5;
const F_FAN: u32 = 1 << 6;
const F_VRAM_USED: u32 = 1 << 7;
const F_VRAM_TOTAL: u32 = 1 << 8;
const F_FPS: u32 = 1 << 9;

/// Flat snapshot from the shim. Layout must match `struct AdlxSample` in C++.
#[repr(C)]
#[derive(Default)]
struct AdlxSample {
    gpu_usage: f64,
    gpu_temp: f64,
    gpu_hotspot: f64,
    gpu_power: f64,
    gpu_clock: i32,
    vram_clock: i32,
    fan_rpm: i32,
    fps: i32,
    vram_used_mb: u32,
    vram_total_mb: u32,
    flags: u32,
}

/// One GPU in the ADLX enumeration. Layout must match `struct AdlxGpuInfo` in C++.
#[repr(C)]
struct AdlxGpuInfo {
    name: [u8; 128],
    kind: i32,
    vram_mb: u32,
}

extern "C" {
    fn adlx_init() -> i32;
    fn adlx_sample(out: *mut AdlxSample, want_fps: i32) -> i32;
    fn adlx_gpu_count() -> i32;
    fn adlx_gpu_info(idx: i32, out: *mut AdlxGpuInfo) -> i32;
    fn adlx_select(idx: i32) -> i32;
    fn adlx_shutdown();
}

/// One enumerated AMD GPU, for the in-app picker.
pub struct GpuListEntry {
    pub name: String,
    /// "Discreta" | "Integrada" | "" (unknown).
    pub kind: String,
    pub vram_mb: u64,
}

/// List the GPUs ADLX can sample (empty if ADLX isn't initialized / no AMD).
pub fn list_gpus() -> Vec<GpuListEntry> {
    let _guard = lock();
    enumerate()
}

/// List the GPUs for a caller that is not the sampler (`system_info`): uses the
/// sampler's ADLX session when there is one, otherwise brings ADLX up only for
/// the enumeration and releases it again.
pub fn list_gpus_transient() -> Vec<GpuListEntry> {
    let guard = lock();
    if *guard {
        return enumerate();
    }
    if unsafe { adlx_init() } != 0 {
        return Vec::new();
    }
    let gpus = enumerate();
    unsafe { adlx_shutdown() };
    gpus
}

/// Caller holds `ADLX_LOCK`.
fn enumerate() -> Vec<GpuListEntry> {
    let count = unsafe { adlx_gpu_count() };
    if count <= 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let mut info = AdlxGpuInfo {
            name: [0; 128],
            kind: 0,
            vram_mb: 0,
        };
        if unsafe { adlx_gpu_info(i, &mut info) } != 0 {
            continue;
        }
        let end = info.name.iter().position(|&b| b == 0).unwrap_or(info.name.len());
        let name = String::from_utf8_lossy(&info.name[..end]).into_owned();
        let kind = match info.kind {
            1 => "Integrada",
            2 => "Discreta",
            _ => "",
        }
        .to_string();
        out.push(GpuListEntry {
            name,
            kind,
            vram_mb: info.vram_mb as u64,
        });
    }
    out
}

/// Select which ADLX GPU `sample` reads (0-based index into `list_gpus`).
pub fn select(idx: usize) -> bool {
    let _guard = lock();
    unsafe { adlx_select(idx as i32) == 0 }
}

/// One GPU telemetry reading; `None` fields are unsupported on this card.
// hotspot/vram_clock/fan are captured by the shim but not yet shown in the HUD;
// kept here so adding overlay rows later needs no shim change.
#[derive(Default)]
#[allow(dead_code)]
pub struct GpuSample {
    pub usage: Option<u32>,
    pub temp_c: Option<u32>,
    pub hotspot_c: Option<u32>,
    pub power_w: Option<f32>,
    pub clock_mhz: Option<u32>,
    pub vram_clock_mhz: Option<u32>,
    pub fan_rpm: Option<u32>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub fps: Option<f32>,
}

/// Initialize ADLX. Returns true on success (AMD GPU + driver present).
pub fn init() -> bool {
    let mut held = lock();
    *held = unsafe { adlx_init() == 0 };
    *held
}

/// Read the current GPU metrics, or `None` if ADLX isn't producing a sample. `want_fps`
/// gates the FPS counter read so we don't pay for it when the overlay shows no FPS row.
pub fn sample(want_fps: bool) -> Option<GpuSample> {
    let _guard = lock();
    let mut s = AdlxSample::default();
    if unsafe { adlx_sample(&mut s, want_fps as i32) } != 0 {
        return None;
    }
    let has = |bit: u32| s.flags & bit != 0;
    Some(GpuSample {
        usage: has(F_USAGE).then_some(s.gpu_usage.round() as u32),
        temp_c: has(F_TEMP).then_some(s.gpu_temp.round() as u32),
        hotspot_c: has(F_HOTSPOT).then_some(s.gpu_hotspot.round() as u32),
        power_w: has(F_POWER).then_some(s.gpu_power as f32),
        clock_mhz: has(F_CLOCK).then_some(s.gpu_clock.max(0) as u32),
        vram_clock_mhz: has(F_VRAM_CLOCK).then_some(s.vram_clock.max(0) as u32),
        fan_rpm: has(F_FAN).then_some(s.fan_rpm.max(0) as u32),
        vram_used_mb: has(F_VRAM_USED).then_some(s.vram_used_mb as u64),
        vram_total_mb: has(F_VRAM_TOTAL).then_some(s.vram_total_mb as u64),
        fps: has(F_FPS).then_some(s.fps.max(0) as f32),
    })
}

/// Tear down ADLX (best-effort; safe to call when not initialized).
#[allow(dead_code)]
pub fn shutdown() {
    let mut held = lock();
    unsafe { adlx_shutdown() };
    *held = false;
}
