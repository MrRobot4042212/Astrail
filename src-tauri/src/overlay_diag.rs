// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Deep, opt-in diagnostics for the in-game overlay (Windows only).
//!
//! Everything here is gated behind the `METEOR_OVERLAY_DEBUG` env var, so in normal
//! use it costs nothing (no per-tick spam — that was removed on purpose). Set
//! `METEOR_OVERLAY_DEBUG=1` before launching (`$env:METEOR_OVERLAY_DEBUG=1; npm run app`)
//! to get a running picture of *why* the overlay does or doesn't cost performance:
//!
//!  - The chosen backend and any init failure.
//!  - The **actual composition mode of our swapchain** — the definitive runtime MPO
//!    test. `OVERLAY` = the HUD is on a hardware overlay plane → the game keeps
//!    independent-flip → zero added latency. `COMPOSED` = DWM is compositing the HUD
//!    (and therefore the game) → the input-lag case. `COMPOSITION_FAILURE` = the
//!    driver refused. This is the single most useful line in the log.
//!  - The foreground window classified as borderless-fullscreen / windowed /
//!    (likely) exclusive, plus its rect vs the monitor — the conditions that decide
//!    whether DWM grants MPO at all.
//!  - The gating decision each time it changes, and a periodic heartbeat with the
//!    live sample.
//!
//! Output goes to stderr **and** to `<app log dir>\overlay-debug.log` so it can be
//! shared after a session.


use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, GWL_STYLE, SM_CMONITORS, WS_CAPTION, WS_THICKFRAME,
};

static ENABLED: OnceLock<bool> = OnceLock::new();
static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// True if `METEOR_OVERLAY_DEBUG` is set to 1/true. Evaluated once.
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var("METEOR_OVERLAY_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// Resolve (once) the log file path under the app log dir. Called from the sampler.
pub fn init(app: &tauri::AppHandle) {
    if !enabled() {
        return;
    }
    LOG_PATH.get_or_init(|| {
        use tauri::Manager;
        let dir = app.path().app_log_dir().ok()?;
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("overlay-debug.log");
        Some(path)
    });
    log("=== overlay debug iniciado (METEOR_OVERLAY_DEBUG=1) ===");
    if let Some(Some(p)) = LOG_PATH.get() {
        log(&format!("log file: {}", p.display()));
    }
}

fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ms = now.as_millis();
    let secs = (ms / 1000) % 86400;
    format!("{:02}:{:02}:{:02}.{:03}", secs / 3600, (secs % 3600) / 60, secs % 60, ms % 1000)
}

/// Write one diagnostic line (stderr + the log file). No-op unless enabled.
pub fn log(msg: &str) {
    if !enabled() {
        return;
    }
    let line = format!("[overlay-diag {}] {}", timestamp(), msg);
    eprintln!("{line}");
    if let Some(Some(path)) = LOG_PATH.get() {
        let _guard = FILE_LOCK.lock();
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Human-readable name for a `DXGI_FRAME_PRESENTATION_MODE` value.
/// 0=COMPOSED, 1=OVERLAY, 2=NONE, 3=COMPOSITION_FAILURE.
pub fn composition_mode_name(mode: i32) -> &'static str {
    match mode {
        0 => "COMPOSED (DWM compone → posible input lag)",
        1 => "OVERLAY (plano hardware/MPO → sin coste)",
        2 => "NONE (sin frames presentados aún)",
        3 => "COMPOSITION_FAILURE (driver rechazó el plano)",
        _ => "DESCONOCIDO",
    }
}

fn read_wstr(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Classify the current foreground window: title, class, owning pid, rect vs its
/// monitor, and whether it looks borderless-fullscreen / windowed. Exclusive
/// fullscreen can't be told apart from borderless purely from the window (it still
/// has an HWND), so it's reported as "borderless/exclusive".
pub fn foreground_report() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return "fg: <ninguna>".to_string();
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let mut title = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut title);
        let title = if n > 0 { read_wstr(&title) } else { String::new() };

        let mut class = [0u16; 256];
        let cn = GetClassNameW(hwnd, &mut class);
        let class = if cn > 0 { read_wstr(&class) } else { String::new() };

        let mut wr = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wr);
        let ww = wr.right - wr.left;
        let wh = wr.bottom - wr.top;

        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let has_caption = style & WS_CAPTION.0 != 0;
        let has_thickframe = style & WS_THICKFRAME.0 != 0;

        // Monitor rect for the fullscreen comparison.
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mw;
        let mh;
        let covers_monitor;
        // MPO context: is the game on the PRIMARY monitor? The native HUD is positioned
        // on the primary monitor, so a game on a secondary display means the HUD never
        // even overlaps it — and multi-monitor commonly disables MPO on some drivers.
        let mut on_primary = false;
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            mw = mi.rcMonitor.right - mi.rcMonitor.left;
            mh = mi.rcMonitor.bottom - mi.rcMonitor.top;
            covers_monitor = wr.left <= mi.rcMonitor.left
                && wr.top <= mi.rcMonitor.top
                && wr.right >= mi.rcMonitor.right
                && wr.bottom >= mi.rcMonitor.bottom;
            // MONITORINFOF_PRIMARY = 0x1.
            on_primary = mi.dwFlags & 0x1 != 0;
        } else {
            mw = 0;
            mh = 0;
            covers_monitor = false;
        }
        let monitors = GetSystemMetrics(SM_CMONITORS);

        let kind = if covers_monitor && !has_caption {
            "BORDERLESS/EXCLUSIVE fullscreen (MPO posible)"
        } else if covers_monitor && has_caption {
            "MAXIMIZADA con borde"
        } else if has_caption || has_thickframe {
            "VENTANA (con borde)"
        } else {
            "sin borde, no cubre monitor"
        };

        format!(
            "fg: pid={pid} «{title}» class={class} win={ww}x{wh}@({},{}) mon={mw}x{mh} caption={has_caption} monitores={monitors} primario={on_primary} → {kind}",
            wr.left, wr.top
        )
    }
}
