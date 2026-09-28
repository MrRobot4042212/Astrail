// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Overlay HUD facade (Windows only).
//!
//! Picks the **MPO-friendly DirectComposition backend** (`overlay_dcomp`) when it
//! initializes, otherwise falls back to the GDI layered window (`overlay_native`).
//! A DirectComposition + DXGI flip swapchain is the surface type DWM can promote to a
//! hardware overlay plane, so the game keeps its independent-flip (low-latency) path;
//! a GDI layered window forces desktop composition. The fallback keeps the HUD working
//! on machines where D3D/DComp init fails.
//!
//! All calls come from the single metrics sampler thread (it owns the HUD window), so
//! backend selection lives in a `thread_local` and the COM objects never cross threads.


use std::cell::Cell;

use crate::metrics::{MetricsSample, MonitorGeometry};
use crate::models::OverlaySettings;

// Backend selection: 0 = not chosen yet, 1 = DirectComposition, 3 = disabled.
//
// There is intentionally **no GDI fallback for the in-game HUD**. A GDI layered
// window (`UpdateLayeredWindow`) is never MPO-eligible, so it *always* drops the
// game from independent-flip to composed-flip → input lag. That's the exact cost
// we're trying to avoid, so if DirectComposition can't init we disable the HUD
// rather than hand the user a guaranteed-laggy overlay. (`overlay_native` stays
// only for the backend-independent foreground helpers below.)
thread_local! {
    static BACKEND: Cell<u8> = const { Cell::new(0) };
}

/// One HUD line: label, formatted value, and the value's RGB color.
pub(crate) struct HudRow {
    pub label: &'static str,
    pub value: String,
    pub rgb: (u8, u8, u8),
}

/// Shown in place of a value that a present source has not delivered yet.
pub(crate) const PLACEHOLDER: &str = "—";

/// Which HUD sources exist this session, decided by the sampler from what the
/// machine has (not from the last reading). A row whose source exists keeps its
/// place with [`PLACEHOLDER`] while a reading is missing; a row without one is
/// left out. Before this every row came and went with its data: the lows needed
/// 100 / 1000 frames, GPU busy a full window, the AMD sidecar its first line,
/// the CPU meter a priming tick, and each change resized the swapchain
/// (`ResizeBuffers` + `SetWindowPos`) and shifted every row below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowSources {
    /// PresentMon can run (elevated or Performance Log Users): FPS, lows,
    /// frametime and GPU busy. Off, FPS may still come from the AMD sidecar in
    /// exclusive fullscreen, so the FPS row stays dynamic then.
    pub presentmon: bool,
    /// A GPU backend was found (NVML or the AMD sidecar): GPU %, GPU °C, VRAM.
    pub gpu: bool,
    /// The sidecar may read the CPU temperature (elevated + PawnIO).
    pub cpu_temp: bool,
}

/// Parse a CSS hex color ("#rrggbb") to (r, g, b). Bad input → white.
///
/// Operates on bytes, never on `&str` slices: the value comes from the settings
/// JSON, so a multi-byte character (e.g. "#ñañaña") would panic a byte-index
/// slice on a char boundary and abort the process (`panic = "abort"`).
pub(crate) fn parse_rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim().trim_start_matches('#').as_bytes();
    if h.len() >= 6 && h[..6].iter().all(u8::is_ascii_hexdigit) {
        let nib = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
        let byte = |i: usize| nib(h[i]) << 4 | nib(h[i + 1]);
        return (byte(0), byte(2), byte(4));
    }
    (255, 255, 255)
}

/// Temperature → color (matches the web HUD): red ≥85, amber ≥75, else emerald.
pub(crate) fn temp_rgb(c: u32) -> (u8, u8, u8) {
    if c >= 85 {
        (0xf8, 0x71, 0x71)
    } else if c >= 75 {
        (0xfb, 0xbf, 0x24)
    } else {
        (0x34, 0xd3, 0x99)
    }
}

/// From this share of the frame time up, the GPU is what limits the frame rate and
/// the row takes the accent colour. Below it the row stays neutral on purpose: a low
/// share means "not the GPU", which can be the CPU, a frame cap or VSync, and this
/// reading cannot tell them apart. Mirrored by `GPU_BOUND_PCT` in `overlayMetrics.ts`.
pub(crate) const GPU_BOUND_PCT: f32 = 95.0;

pub(crate) fn gpu_bound(busy_pct: f32) -> bool {
    busy_pct >= GPU_BOUND_PCT
}

/// Build the HUD title + visible rows from the config + sample (single source of
/// truth shared by both backends, so the metric list never drifts).
pub(crate) fn build_rows(
    cfg: &OverlaySettings,
    m: &MetricsSample,
    sources: RowSources,
) -> (Option<String>, Vec<HudRow>) {
    let accent = parse_rgb(&cfg.accent_color);
    let value = parse_rgb(&cfg.value_color);
    let label = parse_rgb(&cfg.label_color);
    let mut rows: Vec<HudRow> = Vec::new();
    let gb = |mb: u64| format!("{:.1}", mb as f64 / 1024.0);
    // A row is pushed with its value; without one it keeps its place (in the
    // label colour, so a placeholder never reads as a number) only when its
    // source is known to exist this session.
    let mut push = |show: bool, reserved: bool, name: &'static str, row: Option<(String, (u8, u8, u8))>| {
        if !show {
            return;
        }
        match row {
            Some((v, rgb)) => rows.push(HudRow { label: name, value: v, rgb }),
            None if reserved => rows.push(HudRow { label: name, value: PLACEHOLDER.into(), rgb: label }),
            None => {}
        }
    };

    push(cfg.show_fps, sources.presentmon, "FPS", m.fps.map(|f| (format!("{:.0}", f), accent)));
    // PresentMon only; each is backed once the history has 100 / 1000 frames.
    push(cfg.show_lows, sources.presentmon, "1% low", m.fps_low_1.map(|f| (format!("{:.0}", f), value)));
    push(cfg.show_lows, sources.presentmon, "0.1% low", m.fps_low_01.map(|f| (format!("{:.0}", f), value)));
    push(
        cfg.show_frametime,
        sources.presentmon,
        "Frame",
        m.frametime_ms.map(|ft| (format!("{:.1} ms", ft), value)),
    );
    push(
        cfg.show_gpu_busy,
        sources.presentmon,
        "GPU busy",
        m.gpu_busy_pct.map(|p| (format!("{:.0}%", p), if gpu_bound(p) { accent } else { value })),
    );
    push(cfg.show_gpu, sources.gpu, "GPU", m.gpu_usage.map(|u| (format!("{u}%"), accent)));
    push(cfg.show_gpu_temp, sources.gpu, "GPU °C", m.gpu_temp_c.map(|t| (format!("{t}°"), temp_rgb(t))));
    push(
        cfg.show_vram,
        sources.gpu,
        "VRAM",
        m.vram_used_mb.zip(m.vram_total_mb).map(|(u, t)| (format!("{}/{} GB", gb(u), gb(t)), value)),
    );
    // The meter always exists; only its priming tick has no reading (MT6).
    push(cfg.show_cpu, true, "CPU", m.cpu_usage.map(|c| (format!("{:.0}%", c), accent)));
    push(cfg.show_cpu_temp, sources.cpu_temp, "CPU °C", m.cpu_temp_c.map(|t| (format!("{t}°"), temp_rgb(t))));
    if cfg.show_ram {
        rows.push(HudRow {
            label: "RAM",
            value: format!("{}/{} GB", gb(m.ram_used_mb), gb(m.ram_total_mb)),
            rgb: value,
        });
    }

    let title = m.game.as_deref().map(|g| g.to_uppercase());
    (title, rows)
}

/// The frametime graph to draw, if it is switched on and there is one.
pub(crate) fn graph_points<'a>(cfg: &OverlaySettings, m: &'a MetricsSample) -> Option<&'a [f32]> {
    if !cfg.show_frametime_graph {
        return None;
    }
    m.frametime_graph.as_ref().map(|g| g.points()).filter(|p| p.len() >= 2)
}

/// One bar of the frametime graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphBar {
    /// Height as a fraction of the graph, in `(0, 1]`.
    pub height: f32,
    /// Clearly slower than the rest of the graph: drawn in the accent color.
    pub spike: bool,
}

/// What the graph is scaled against: the median of its points. The graph has no axis
/// (the numbers are in the rows above it), so its job is to show *shape*: the median
/// sits at half height, and it does not move when a single hitch comes in, which an
/// average would.
pub(crate) fn graph_baseline_ms(points: &[f32]) -> f32 {
    let mut sorted = [0.0f32; crate::presentmon::GRAPH_POINTS];
    let n = points.len().min(sorted.len());
    if n == 0 {
        return 0.0;
    }
    sorted[..n].copy_from_slice(&points[..n]);
    *sorted[..n].select_nth_unstable_by(n / 2, f32::total_cmp).1
}

/// A frametime against the baseline: full height is twice the baseline (anything
/// slower is clipped there), and 1.5× the baseline is a spike.
pub(crate) fn graph_bar(frametime_ms: f32, baseline_ms: f32) -> GraphBar {
    if baseline_ms <= 0.0 || !frametime_ms.is_finite() {
        return GraphBar { height: GRAPH_MIN_BAR, spike: false };
    }
    let ratio = frametime_ms / baseline_ms;
    GraphBar { height: (ratio / 2.0).clamp(GRAPH_MIN_BAR, 1.0), spike: ratio > 1.5 }
}

/// A bar never disappears: a very fast slice still leaves a mark.
const GRAPH_MIN_BAR: f32 = 0.06;

/// Top-left corner of a `w`×`h` HUD placed in `position` on `mon`, in virtual-desktop
/// coordinates.
///
/// Monitor coordinates are not zero-based: a display left of or above the primary
/// has a negative origin, one to the right starts at the primary's width. The old
/// math worked in `[0, mon_w)` and so always drew on the primary monitor.
pub(crate) fn hud_origin(
    position: crate::models::OverlayPosition,
    mon: MonitorGeometry,
    w: i32,
    h: i32,
    margin: i32,
) -> (i32, i32) {
    use crate::models::OverlayPosition;
    let (right, bottom) = (mon.left + mon.width, mon.top + mon.height);
    let (x, y) = match position {
        OverlayPosition::TopLeft => (mon.left + margin, mon.top + margin),
        OverlayPosition::TopRight => (right - w - margin, mon.top + margin),
        OverlayPosition::BottomLeft => (mon.left + margin, bottom - h - margin),
        OverlayPosition::BottomRight => (right - w - margin, bottom - h - margin),
    };
    // Never start outside the monitor when the HUD is wider/taller than it.
    (x.max(mon.left), y.max(mon.top))
}

fn current() -> u8 {
    BACKEND.with(|b| b.get())
}

/// Draw + present the HUD via DirectComposition. If DComp can't init (or fails at
/// runtime) the HUD is disabled for the session — no GDI fallback, so we never
/// force composition on the game (see the BACKEND comment).
pub fn render(cfg: &OverlaySettings, m: &MetricsSample, monitor: MonitorGeometry, sources: RowSources) {
    let mut b = current();
    if b == 0 {
        b = if crate::overlay_dcomp::try_init() {
            crate::overlay_diag::log("backend: DirectComposition (MPO-friendly)");
            log::info!("HUD backend ready: DirectComposition");
            1
        } else {
            crate::overlay_diag::log(
                "DirectComposition unavailable → HUD disabled (no GDI fallback)",
            );
            3
        };
        BACKEND.with(|c| c.set(b));
    }
    if b == 1 && !crate::overlay_dcomp::render(cfg, m, monitor, sources) {
        // Runtime failure: disable the HUD for the rest of the session.
        crate::overlay_dcomp::hide();
        BACKEND.with(|c| c.set(3));
    }
}

/// Hide the HUD (no-op if no backend has rendered yet).
pub fn hide() {
    if current() == 1 {
        crate::overlay_dcomp::hide();
    }
}

/// Release the HUD backend entirely (window + GPU objects). The next `render`
/// re-initializes it. No-op unless the DComp backend is live.
pub fn teardown() {
    if current() == 1 {
        crate::overlay_dcomp::teardown();
        BACKEND.with(|c| c.set(0));
    }
}

/// Drain the HUD window's pending messages (no-op until a backend exists).
pub fn pump() {
    if current() == 1 {
        crate::overlay_dcomp::pump();
    }
}

/// Re-assert topmost after a foreground change (no-op until a backend exists).
pub fn reassert_topmost() {
    if current() == 1 {
        crate::overlay_dcomp::reassert_topmost();
    }
}

/// Raw composition mode of the HUD swapchain (0=COMPOSED, 1=OVERLAY, 2=NONE,
/// 3=FAILURE), or `None` if not measurable. The runtime MPO signal: OVERLAY = the HUD
/// is on a hardware plane (free), COMPOSED = DWM is compositing it (costing the game).
pub fn composition_mode() -> Option<i32> {
    if current() == 1 {
        crate::overlay_dcomp::composition_mode()
    } else {
        None
    }
}

/// Log the swapchain's actual composition mode (OVERLAY=hardware plane vs
/// COMPOSED=DWM compositing) — the definitive runtime MPO check. No-op unless the
/// DComp backend is active and diagnostics are enabled.
pub fn log_composition_mode() {
    if current() == 1 {
        crate::overlay_dcomp::log_composition_mode();
    } else {
        crate::overlay_diag::log("composition: HUD not active on the DComp backend");
    }
}

/// Foreground window handle (0 = none); backend-independent.
pub fn foreground() -> isize {
    crate::overlay_native::foreground()
}

/// PID owning the foreground window (0 = none); backend-independent.
pub fn foreground_pid() -> u32 {
    crate::overlay_native::foreground_pid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OverlayPosition;

    fn primary() -> MonitorGeometry {
        MonitorGeometry { left: 0, top: 0, width: 1920, height: 1080, scale: 1.0 }
    }

    #[test]
    fn hud_is_placed_on_the_game_monitor_not_the_primary() {
        // Regression (W3): a game on a secondary monitor got its HUD on the primary.
        let right_of_primary = MonitorGeometry { left: 1920, top: 0, width: 2560, height: 1440, scale: 1.0 };
        assert_eq!(hud_origin(OverlayPosition::TopLeft, right_of_primary, 160, 100, 12), (1932, 12));
        assert_eq!(hud_origin(OverlayPosition::BottomRight, right_of_primary, 160, 100, 12), (4480 - 172, 1440 - 112));

        let left_of_primary = MonitorGeometry { left: -1280, top: -200, width: 1280, height: 1024, scale: 1.0 };
        assert_eq!(hud_origin(OverlayPosition::TopRight, left_of_primary, 160, 100, 12), (-172, -188));
        assert_eq!(hud_origin(OverlayPosition::BottomLeft, left_of_primary, 160, 100, 12), (-1268, 824 - 112));
    }

    #[test]
    fn hud_on_the_primary_keeps_its_old_position() {
        assert_eq!(hud_origin(OverlayPosition::TopLeft, primary(), 160, 100, 12), (12, 12));
        assert_eq!(hud_origin(OverlayPosition::TopRight, primary(), 160, 100, 12), (1748, 12));
    }

    #[test]
    fn an_oversized_hud_stays_inside_its_monitor() {
        let m = MonitorGeometry { left: 1920, top: 0, width: 100, height: 50, scale: 1.0 };
        assert_eq!(hud_origin(OverlayPosition::BottomRight, m, 400, 300, 12), (1920, 0));
    }

    fn sample(cpu: Option<f32>) -> MetricsSample {
        MetricsSample {
            game: None,
            cpu_usage: cpu,
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
            fps_low_1: None,
            fps_low_01: None,
            frametime_graph: None,
            gpu_busy_pct: None,
        }
    }

    #[test]
    fn the_gpu_busy_row_is_opt_in_and_only_accents_a_gpu_limit() {
        let on = OverlaySettings { show_gpu_busy: true, ..OverlaySettings::default() };
        let accent = parse_rgb(&on.accent_color);
        let value = parse_rgb(&on.value_color);
        assert_ne!(accent, value, "the test needs two colours to tell apart");
        let mut m = sample(None);
        m.frametime_ms = Some(7.0);
        let row = |cfg: &OverlaySettings, m: &MetricsSample| {
            build_rows(cfg, m, RowSources::default()).1.into_iter().find(|r| r.label == "GPU busy")
        };
        assert!(row(&on, &m).is_none(), "no reading, no row");
        m.gpu_busy_pct = Some(62.4);
        assert!(row(&OverlaySettings::default(), &m).is_none(), "off by default");
        let low = row(&on, &m).expect("row");
        assert_eq!((low.value.as_str(), low.rgb), ("62%", value));
        m.gpu_busy_pct = Some(97.2);
        let high = row(&on, &m).expect("row");
        assert_eq!((high.value.as_str(), high.rgb), ("97%", accent));
        // Right after the frametime it explains (CPU keeps its slot: see
        // `cpu_row_never_shows_a_made_up_number`).
        let labels: Vec<_> = build_rows(&on, &m, RowSources::default()).1.iter().map(|r| r.label).collect();
        assert_eq!(labels, ["Frame", "GPU busy", "CPU", "RAM"]);
    }

    #[test]
    fn low_rows_follow_the_data_and_the_switch() {
        let cfg = OverlaySettings::default();
        let labels = |cfg: &OverlaySettings, m: &MetricsSample| {
            build_rows(cfg, m, RowSources::default()).1.iter().map(|r| r.label).collect::<Vec<_>>()
        };
        let mut m = sample(None);
        m.fps = Some(143.6);
        assert_eq!(labels(&cfg, &m), ["FPS", "CPU", "RAM"], "no lows without enough frames");
        m.fps_low_1 = Some(97.4);
        assert_eq!(labels(&cfg, &m), ["FPS", "1% low", "CPU", "RAM"]);
        m.fps_low_01 = Some(61.5);
        let (_, rows) = build_rows(&cfg, &m, RowSources::default());
        assert_eq!(rows[1].value, "97");
        assert_eq!(rows[2].label, "0.1% low");
        assert_eq!(rows[2].value, "62");
        let off = OverlaySettings { show_lows: false, ..OverlaySettings::default() };
        assert_eq!(labels(&off, &m), ["FPS", "CPU", "RAM"]);
    }

    #[test]
    fn the_graph_is_drawn_only_when_asked_for_and_available() {
        use crate::presentmon::FrameGraph;
        let on = OverlaySettings { show_frametime_graph: true, ..OverlaySettings::default() };
        let mut m = sample(None);
        assert_eq!(graph_points(&on, &m), None);
        m.frametime_graph = Some(FrameGraph::from_points(&[7.0]));
        assert_eq!(graph_points(&on, &m), None, "one point is not a graph");
        m.frametime_graph = Some(FrameGraph::from_points(&[7.0, 8.0]));
        assert_eq!(graph_points(&on, &m), Some(&[7.0, 8.0][..]));
        assert_eq!(graph_points(&OverlaySettings::default(), &m), None, "off by default");
    }

    #[test]
    fn graph_bars_scale_against_the_median() {
        // One hitch does not move the baseline, so the steady bars stay put.
        let mut points = vec![7.0f32; 59];
        points.push(40.0);
        let base = graph_baseline_ms(&points);
        assert_eq!(base, 7.0);
        assert_eq!(graph_bar(7.0, base), GraphBar { height: 0.5, spike: false });
        assert!(!graph_bar(10.0, base).spike, "1.4x the baseline is not a spike");
        assert_eq!(graph_bar(40.0, base), GraphBar { height: 1.0, spike: true });
        assert!(graph_bar(0.1, base).height > 0.0);
        // Nothing to scale against, or garbage: a minimal bar, never a NaN rect.
        assert_eq!(graph_baseline_ms(&[]), 0.0);
        assert!(graph_bar(7.0, 0.0).height > 0.0);
        assert!(graph_bar(f32::NAN, 7.0).height.is_finite());
    }

    #[test]
    fn cpu_row_never_shows_a_made_up_number() {
        // Regression (MT6): the priming tick drew `CPU 0%`. It now keeps its place
        // with the placeholder, so the rows below do not shift on the second tick.
        let cfg = OverlaySettings { show_cpu: true, ..OverlaySettings::default() };
        let (_, rows) = build_rows(&cfg, &sample(None), RowSources::default());
        let cpu = rows.iter().find(|r| r.label == "CPU").expect("cpu row");
        assert_eq!((cpu.value.as_str(), cpu.rgb), (PLACEHOLDER, parse_rgb(&cfg.label_color)));
        let (_, rows) = build_rows(&cfg, &sample(Some(37.4)), RowSources::default());
        let cpu = rows.iter().find(|r| r.label == "CPU").expect("cpu row");
        assert_eq!(cpu.value, "37%");
    }

    #[test]
    fn rows_with_a_known_source_keep_their_place_with_a_placeholder() {
        // Regression: every row came and went with its reading (lows after 100 /
        // 1000 frames, GPU busy after half a window, the AMD sidecar's first line,
        // a loading screen expiring the FPS rows), and each change resized the HUD.
        let cfg = OverlaySettings {
            show_gpu_busy: true,
            show_cpu_temp: true,
            ..OverlaySettings::default()
        };
        let all = RowSources { presentmon: true, gpu: true, cpu_temp: true };
        let labels = |m: &MetricsSample, s: RowSources| {
            build_rows(&cfg, m, s).1.iter().map(|r| r.label).collect::<Vec<_>>()
        };
        let empty = sample(None);
        // Nothing measured yet: the whole layout is already there.
        assert_eq!(
            labels(&empty, all),
            ["FPS", "1% low", "0.1% low", "Frame", "GPU busy", "GPU", "GPU °C", "VRAM", "CPU", "CPU °C", "RAM"]
        );
        let (_, rows) = build_rows(&cfg, &empty, all);
        assert!(rows.iter().filter(|r| r.label != "RAM").all(|r| r.value == PLACEHOLDER));
        // Readings fill the same slots: same labels, same order.
        let mut m = sample(Some(12.0));
        m.fps = Some(143.6);
        m.fps_low_1 = Some(97.4);
        m.gpu_usage = Some(80);
        m.cpu_temp_c = Some(61);
        assert_eq!(labels(&m, all), labels(&empty, all));
        let (_, rows) = build_rows(&cfg, &m, all);
        assert_eq!(rows[0].value, "144");
        assert_eq!(rows[1].value, "97");
        assert_eq!(rows[2].value, PLACEHOLDER, "0.1 % low still unbacked");
        // A source that does not exist on this machine leaves no placeholder.
        let none = RowSources::default();
        assert_eq!(labels(&empty, none), ["CPU", "RAM"]);
        // Without a known source a row still shows when a reading is there (the
        // sidecar's opportunistic FPS, a CPU temperature that arrived anyway) and
        // goes when it is not (no lows without PresentMon).
        m.fps_low_1 = None;
        assert_eq!(
            labels(&m, RowSources { gpu: true, ..none }),
            ["FPS", "GPU", "GPU °C", "VRAM", "CPU", "CPU °C", "RAM"]
        );
        m.fps = None;
        m.cpu_temp_c = None;
        assert_eq!(labels(&m, RowSources { gpu: true, ..none }), ["GPU", "GPU °C", "VRAM", "CPU", "RAM"]);
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_rgb("#ff8800"), (0xff, 0x88, 0x00));
        assert_eq!(parse_rgb("  00FF7F  "), (0x00, 0xff, 0x7f));
        // Extra characters after the 6 hex digits are ignored (e.g. #rrggbbaa).
        assert_eq!(parse_rgb("#10203040"), (0x10, 0x20, 0x30));
    }

    #[test]
    fn bad_input_falls_back_to_white_without_panicking() {
        // Regression (H7): byte-slicing "#ñañaña" split a multi-byte char and
        // aborted the process, since the release profile uses panic = "abort".
        assert_eq!(parse_rgb("#ñañaña"), (255, 255, 255));
        assert_eq!(parse_rgb("鏡鏡鏡"), (255, 255, 255));
        assert_eq!(parse_rgb("#zzzzzz"), (255, 255, 255));
        assert_eq!(parse_rgb("#fff"), (255, 255, 255));
        assert_eq!(parse_rgb(""), (255, 255, 255));
    }
}
