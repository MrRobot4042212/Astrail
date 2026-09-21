// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! FPS / frametime via **PresentMon** (Intel/Microsoft, ETW-based — no DLL
//! injection, so anti-cheat safe). A controller thread spawns `PresentMon.exe`
//! targeting the running game's PID, streams its CSV from stdout, and keeps a
//! ~1s rolling window of frame times to derive FPS and average frametime.
//!
//! The same stream feeds the 1 % / 0.1 % lows and the frametime graph. Both are
//! built on the reader thread from a fixed-size history of the reported swapchain
//! (no allocation once the parser exists) and handed to the sampler through
//! atomics and one small mutex; the sampler never sees the frame stream itself.
//!
//! Requirements (both needed for FPS to appear; everything degrades silently to
//! `None` otherwise, so the rest of the overlay always works):
//!   1. The `PresentMon.exe` binary present (see `binaries/README.md`).
//!   2. An ETW realtime session: Astrail running **elevated**, or the user in the
//!      built-in Performance Log Users group (`elevation::can_trace_etw`). Without
//!      admin PresentMon warns that processes of *other accounts* show up as
//!      `<unknown>`; we target by `--process_id` and watch the game's exit
//!      ourselves, so neither limit applies to a game the user started.

use serde::{Serialize, Serializer};
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
/// 1 % / 0.1 % low FPS as hundredths (0 = not enough frames yet).
static LOW_1_X100: AtomicU32 = AtomicU32::new(0);
static LOW_01_X100: AtomicU32 = AtomicU32::new(0);
/// GPU busy share of the frame time as tenths of a percent, plus one (0 = no data,
/// so a measured 0.0 % is still distinguishable from "nothing to report").
static GPU_BUSY_X10: AtomicU32 = AtomicU32::new(0);
/// `metrics::clock_ms()` of the last published frame (0 = never).
static LAST_UPDATE_MS: AtomicU64 = AtomicU64::new(0);
/// Latest frametime graph. Written by the reader thread once per finished slice,
/// copied out by the sampler once per tick.
static GRAPH: Mutex<FrameGraph> = Mutex::new(FrameGraph::EMPTY);

/// Our own ETW session name. PresentMon defaults to a fixed well-known name, which
/// is why `--stop_existing_session` used to be needed — and why it could tear down
/// a session belonging to another PresentMon consumer (CapFrameX, Intel's own
/// service, the user's own run). With a private name we only ever stop our own.
const SESSION_NAME: &str = "Astrail-PresentMon";

/// The running child, shared with `shutdown()` so a clean app exit can stop the
/// ETW session instead of leaving the Job Object to terminate the process.
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// Ceiling for a graceful stop before we terminate and fall back to stopping the
/// ETW session ourselves.
const GRACEFUL_STOP: Duration = Duration::from_millis(1500);

fn child_lock() -> MutexGuard<'static, Option<Child>> {
    CHILD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What PresentMon measured for the running game. Every field is `None` until there
/// is enough fresh data to back it: nothing here is ever derived or guessed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    pub fps: Option<f32>,
    pub frametime_ms: Option<f32>,
    /// See [`percentile_lows`] for the exact definition.
    pub low_1: Option<f32>,
    pub low_01: Option<f32>,
    /// See [`FrameParser::gpu_busy_pct`] for the exact definition.
    pub gpu_busy_pct: Option<f32>,
}

fn is_live() -> bool {
    let stamp = LAST_UPDATE_MS.load(Ordering::Relaxed);
    crate::metrics::is_fresh(stamp, crate::metrics::clock_ms(), crate::metrics::fps_max_age_ms())
}

/// Current frame statistics, if PresentMon produced them recently.
///
/// The reader only writes when a frame arrives, so a game that stops presenting
/// (loading screen, hang, minimized) must expire here instead of freezing its last
/// FPS on the HUD.
pub fn current() -> FrameStats {
    if !is_live() {
        return FrameStats::default();
    }
    let opt = |v: &AtomicU32| match v.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v as f32 / 100.0),
    };
    FrameStats {
        fps: opt(&FPS_X100),
        frametime_ms: opt(&FRAMETIME_X100),
        low_1: opt(&LOW_1_X100),
        low_01: opt(&LOW_01_X100),
        gpu_busy_pct: match GPU_BUSY_X10.load(Ordering::Relaxed) {
            0 => None,
            v => Some((v - 1) as f32 / 10.0),
        },
    }
}

/// The frametime graph, under the same freshness rule as [`current`]. `None` until
/// there are two finished slices to draw.
pub fn graph() -> Option<FrameGraph> {
    if !is_live() {
        return None;
    }
    let graph = *GRAPH.lock().unwrap_or_else(PoisonError::into_inner);
    (graph.len >= 2).then_some(graph)
}

fn reset() {
    FPS_X100.store(0, Ordering::Relaxed);
    FRAMETIME_X100.store(0, Ordering::Relaxed);
    LOW_1_X100.store(0, Ordering::Relaxed);
    LOW_01_X100.store(0, Ordering::Relaxed);
    GPU_BUSY_X10.store(0, Ordering::Relaxed);
    LAST_UPDATE_MS.store(0, Ordering::Relaxed);
    *GRAPH.lock().unwrap_or_else(PoisonError::into_inner) = FrameGraph::EMPTY;
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
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    // Kill-on-close job: if Astrail dies for any reason (crash, force-quit,
    // `panic = "abort"`), the kernel terminates this elevated child and its ETW
    // session instead of leaving it orphaned.
    #[cfg(windows)]
    crate::jobobj::assign(&child);
    if let Some(err) = child.stderr.take() {
        crate::sidecar_log::forward("PresentMon", err);
    }
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || parse_stdout(out, pid));
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

/// Frames the lows are computed over, at most. Allocated once per parser and never
/// grown: above ~270 fps the history covers less than `HISTORY_WINDOW_MS`.
const HISTORY_CAP: usize = 8192;
/// The lows describe the last 30 s of play, not the whole session: a HUD number has
/// to follow what the game is doing now.
const HISTORY_WINDOW_MS: f64 = 30_000.0;
/// A frame longer than this is the game not presenting (loading screen, alt-tab,
/// pause), not a slow frame. Same threshold as `CHAIN_TTL_MS`. It still counts for
/// the FPS window, but it stays out of the lows and the graph, where a single one
/// would otherwise own the 0.1 % low for the next 30 s.
const GAP_MS: f32 = 1000.0;
/// Fewer frames than this and a percentile is one or two frames wide: not shown.
const MIN_FRAMES_LOW_1: usize = 100;
const MIN_FRAMES_LOW_01: usize = 1000;
/// How often the reader thread recomputes the lows.
const LOWS_EVERY_MS: u64 = 500;
/// Frames the reader holds for the session summary before handing them over. It
/// does so on the `LOWS_EVERY_MS` cadence; at 800 fps that is 400 frames, so the
/// batch only fills up when stdout arrives in a burst.
const SESSION_BATCH: usize = 1024;
/// The GPU busy share needs this much of the window filled, and this many frames,
/// before it is a share of anything: one long frame is not a trend.
const MIN_BUSY_WINDOW_MS: f32 = 500.0;
const MIN_BUSY_FRAMES: usize = 10;
/// Points in the frametime graph.
pub const GRAPH_POINTS: usize = 60;
/// Game time one graph point covers; 60 of them are the last 12 s.
const GRAPH_SLICE_MS: f32 = 200.0;

/// The frametime graph: the worst frametime (ms) of each finished
/// `GRAPH_SLICE_MS` slice of game time, oldest first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameGraph {
    points: [f32; GRAPH_POINTS],
    len: usize,
}

impl FrameGraph {
    pub const EMPTY: Self = Self { points: [0.0; GRAPH_POINTS], len: 0 };

    /// Finished slices, oldest first.
    pub fn points(&self) -> &[f32] {
        &self.points[..self.len]
    }

    fn push(&mut self, worst_ms: f32) {
        if self.len == GRAPH_POINTS {
            self.points.copy_within(1.., 0);
            self.len -= 1;
        }
        self.points[self.len] = worst_ms;
        self.len += 1;
    }

    #[cfg(test)]
    pub(crate) fn from_points(points: &[f32]) -> Self {
        let mut graph = Self::EMPTY;
        for p in points {
            graph.push(*p);
        }
        graph
    }
}

// serde implements `Serialize` for arrays only up to 32 elements, and the unused
// tail is not data anyway.
impl Serialize for FrameGraph {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.points())
    }
}

/// 1 % and 0.1 % low FPS of a set of frametimes (ms), as percentiles: the 1 % low is
/// the FPS of the frametime that 99 % of the frames beat — the *fastest* of the worst
/// 1 % — and the 0.1 % low is the same at 99.9 %. This is the percentile reading
/// (CapFrameX "P1" / "P0.1"), not the average of the worst 1 %, which is lower and
/// swings with a single outlier. Reorders `frametimes`.
fn percentile_lows(frametimes: &mut [f32]) -> (Option<f32>, Option<f32>) {
    let n = frametimes.len();
    if n < MIN_FRAMES_LOW_1 {
        return (None, None);
    }
    let fps = |ft: f32| (ft > 0.0).then(|| 1000.0 / ft);
    let worst_1 = n / 100;
    let (_, p99, slower) = frametimes.select_nth_unstable_by(n - worst_1, f32::total_cmp);
    let low_1 = fps(*p99);
    if n < MIN_FRAMES_LOW_01 {
        return (low_1, None);
    }
    // `slower` is the `worst_1 - 1` frames above the 99th percentile; the 99.9th
    // sits `n / 1000` from its end.
    let low_01 = slower
        .len()
        .checked_sub(n / 1000)
        .filter(|i| *i < slower.len())
        .and_then(|i| fps(*slower.select_nth_unstable_by(i, f32::total_cmp).1));
    (low_1, low_01)
}

/// Frametimes of the reported swapchain: the source of the lows and the graph.
struct History {
    frames: VecDeque<f32>,
    total_ms: f64,
    graph: FrameGraph,
    /// Bumped whenever `graph` changes, so the reader only publishes then.
    graph_version: u32,
    /// Game time accumulated in the slice being filled, and its worst frame so far.
    slice_ms: f32,
    slice_worst: f32,
    /// Set by `clear`, so lows of a swapchain that is gone are withdrawn at once.
    cleared: bool,
}

impl History {
    fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(HISTORY_CAP),
            total_ms: 0.0,
            graph: FrameGraph::EMPTY,
            graph_version: 0,
            slice_ms: 0.0,
            slice_worst: 0.0,
            cleared: false,
        }
    }

    fn clear(&mut self) {
        self.frames.clear();
        self.total_ms = 0.0;
        self.graph = FrameGraph::EMPTY;
        self.graph_version = self.graph_version.wrapping_add(1);
        self.slice_ms = 0.0;
        self.slice_worst = 0.0;
        self.cleared = true;
    }

    fn push(&mut self, ft: f32) {
        if ft > GAP_MS {
            return;
        }
        // Never grows: the oldest frame leaves before the new one enters.
        if self.frames.len() == HISTORY_CAP {
            if let Some(old) = self.frames.pop_front() {
                self.total_ms -= f64::from(old);
            }
        }
        self.frames.push_back(ft);
        self.total_ms += f64::from(ft);
        while self.total_ms > HISTORY_WINDOW_MS && self.frames.len() > 1 {
            if let Some(old) = self.frames.pop_front() {
                self.total_ms -= f64::from(old);
            }
        }

        // The graph runs on game time (the sum of frametimes), not on the time a row
        // was read: stdout arrives in bursts, the frametimes do not.
        self.slice_ms += ft;
        if self.slice_ms < GRAPH_SLICE_MS {
            self.slice_worst = self.slice_worst.max(ft);
            return;
        }
        // This frame ends in a later slice. The slice it started in shows the frames
        // that ended inside it; slices nothing ended in were spent waiting for this
        // frame, so they show it.
        let finished = (self.slice_ms / GRAPH_SLICE_MS) as usize;
        self.graph.push(if self.slice_worst > 0.0 { self.slice_worst } else { ft });
        for _ in 1..finished.min(GRAPH_POINTS) {
            self.graph.push(ft);
        }
        self.slice_ms = (self.slice_ms - finished as f32 * GRAPH_SLICE_MS).max(0.0);
        self.slice_worst = ft;
        self.graph_version = self.graph_version.wrapping_add(1);
    }
}

/// Rolling ~1 s window of one swapchain's frame times.
struct Chain {
    id: u64,
    frames: VecDeque<f32>,
    sum: f32,
    /// `msGPUActive` of the same frames, in lockstep with `frames`; a frame whose
    /// row carried no usable value holds `GPU_UNKNOWN`.
    gpu: VecDeque<f32>,
    gpu_sum: f32,
    /// Frames in the window that do carry a value.
    gpu_known: usize,
    last_ms: u64,
}

/// Marks a frame without a GPU reading. Negative, so it can never be a real one.
const GPU_UNKNOWN: f32 = -1.0;

/// Parses PresentMon's CSV and keeps one frame window **per swapchain**.
///
/// A game process often owns more than one swapchain (an embedded Chromium
/// launcher/UI, a secondary window, a video layer). PresentMon emits one row per
/// present for all of them, and `msBetweenPresents` is measured per swapchain, so
/// folding every row into one window mixed a 144 fps game with a 30 fps UI into a
/// single inflated number. The reported value is the busiest live swapchain, which
/// is the one rendering the game.
///
/// The lows and the graph need one continuous series, so the reported swapchain is
/// sticky: it stays the owner of `history` until another one is clearly busier (see
/// `reported`). Two swapchains presenting at the same rate would otherwise trade
/// places on every frame and restart the history each time.
pub(crate) struct FrameParser {
    ft_col: Option<usize>,
    sc_col: Option<usize>,
    /// `msGPUActive`. Optional: PresentMon only writes it while it tracks GPU work.
    gpu_col: Option<usize>,
    chains: Vec<Chain>,
    /// Swapchain whose frames `history` holds.
    owner: Option<u64>,
    history: History,
    /// Reused copy of the history for the percentile selection, which reorders it.
    scratch: Vec<f32>,
    /// Whether frames of the reported swapchain are also kept for the session
    /// summary (`sessionperf`). Set by the reader from the sampler's gate.
    collect: bool,
    /// Those frames, until the reader hands them over. Never grows: the reader
    /// drains it before it reaches `SESSION_BATCH`.
    session: Vec<f32>,
}

impl FrameParser {
    pub(crate) fn new() -> Self {
        Self {
            ft_col: None,
            sc_col: None,
            gpu_col: None,
            chains: Vec::new(),
            owner: None,
            history: History::new(),
            scratch: Vec::with_capacity(HISTORY_CAP),
            collect: false,
            session: Vec::with_capacity(SESSION_BATCH),
        }
    }

    pub(crate) fn set_collect(&mut self, on: bool) {
        self.collect = on;
    }

    pub(crate) fn session_batch_full(&self) -> bool {
        self.session.len() >= SESSION_BATCH
    }

    /// Hand the collected frames to `sink` and start a new batch.
    pub(crate) fn drain_session(&mut self, sink: impl FnOnce(&[f32])) {
        if !self.session.is_empty() {
            sink(&self.session);
            self.session.clear();
        }
    }

    /// Index of the swapchain to report: the owner of the history while it is live
    /// and within 10 % of the busiest one, otherwise the busiest (most presents in
    /// its last second, ties to the most recent), which then becomes the owner of a
    /// fresh history.
    fn reported(&mut self) -> Option<usize> {
        let busiest = (0..self.chains.len())
            .max_by_key(|&i| (self.chains[i].frames.len(), self.chains[i].last_ms))?;
        let owner = self.owner.and_then(|id| self.chains.iter().position(|c| c.id == id));
        if let Some(o) = owner {
            if self.chains[o].frames.len() * 10 >= self.chains[busiest].frames.len() * 9 {
                return Some(o);
            }
        }
        self.owner = Some(self.chains[busiest].id);
        self.history.clear();
        Some(busiest)
    }

    /// 1 % / 0.1 % low FPS over the history, each `None` until it has enough frames.
    pub(crate) fn lows(&mut self) -> (Option<f32>, Option<f32>) {
        let (a, b) = self.history.frames.as_slices();
        self.scratch.clear();
        self.scratch.extend_from_slice(a);
        self.scratch.extend_from_slice(b);
        percentile_lows(&mut self.scratch)
    }

    pub(crate) fn graph(&self) -> FrameGraph {
        self.history.graph
    }

    pub(crate) fn graph_version(&self) -> u32 {
        self.history.graph_version
    }

    /// True once after the history restarted: published lows belong to a swapchain
    /// that is no longer the reported one.
    pub(crate) fn take_cleared(&mut self) -> bool {
        std::mem::take(&mut self.history.cleared)
    }

    /// Share of the reported swapchain's last second that the GPU spent working on
    /// its frames: `sum(msGPUActive) / sum(msBetweenPresents)`, as a percentage.
    ///
    /// Near 100 % the GPU is what limits the frame rate. A low share only says the
    /// GPU is **not** the limit; it cannot tell a CPU limit from a frame cap or
    /// VSync, so nothing here names the CPU.
    ///
    /// `None` unless every frame in the window carried a reading, the window is at
    /// least half full, and the GPU did some work in it: a PresentMon that cannot see
    /// GPU events writes 0 on every row, which is "unknown", not "0 % busy". GPU work
    /// of pipelined frames overlaps, so the sum can pass the frame time: capped at 100.
    pub(crate) fn gpu_busy_pct(&self) -> Option<f32> {
        let owner = self.owner?;
        let chain = self.chains.iter().find(|c| c.id == owner)?;
        let complete = chain.gpu_known == chain.frames.len()
            && chain.frames.len() >= MIN_BUSY_FRAMES
            && chain.sum >= MIN_BUSY_WINDOW_MS;
        (complete && chain.gpu_sum > 0.0).then(|| (chain.gpu_sum / chain.sum * 100.0).min(100.0))
    }

    /// Feed one CSV line read at `now_ms`. Returns `(fps, avg_frametime_ms)` of the
    /// reported swapchain after a valid data row, `None` otherwise.
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
                } else if c == "msgpuactive" {
                    self.gpu_col = Some(i);
                }
            }
            return None;
        };

        // Data rows (the hot path at 200-800 fps): one pass over the fields, no
        // per-line allocation.
        let mut ft: Option<f32> = None;
        let mut chain_id: u64 = 0;
        let mut gpu: Option<f32> = None;
        for (i, field) in line.split(',').enumerate() {
            if i == ft_idx {
                ft = field.trim().parse::<f32>().ok();
            } else if Some(i) == self.sc_col {
                chain_id = parse_address(field);
            } else if Some(i) == self.gpu_col {
                gpu = field.trim().parse::<f32>().ok();
            }
        }
        let ft = ft.filter(|v| v.is_finite() && *v > 0.0)?;
        let gpu = gpu.filter(|v| v.is_finite() && *v >= 0.0);

        self.chains.retain(|c| now_ms.saturating_sub(c.last_ms) <= CHAIN_TTL_MS);
        let pos = match self.chains.iter().position(|c| c.id == chain_id) {
            Some(p) => p,
            None => {
                if self.chains.len() >= MAX_CHAINS {
                    if let Some(oldest) = (0..self.chains.len()).min_by_key(|&i| self.chains[i].last_ms) {
                        self.chains.swap_remove(oldest);
                    }
                }
                self.chains.push(Chain {
                    id: chain_id,
                    frames: VecDeque::new(),
                    sum: 0.0,
                    gpu: VecDeque::new(),
                    gpu_sum: 0.0,
                    gpu_known: 0,
                    last_ms: now_ms,
                });
                self.chains.len() - 1
            }
        };

        let chain = &mut self.chains[pos];
        chain.last_ms = now_ms;
        chain.frames.push_back(ft);
        chain.sum += ft;
        chain.gpu.push_back(gpu.unwrap_or(GPU_UNKNOWN));
        if let Some(g) = gpu {
            chain.gpu_sum += g;
            chain.gpu_known += 1;
        }
        while chain.sum > WINDOW_MS && chain.frames.len() > 1 {
            if let Some(old) = chain.frames.pop_front() {
                chain.sum -= old;
            }
            if let Some(old) = chain.gpu.pop_front().filter(|g| *g >= 0.0) {
                chain.gpu_sum = (chain.gpu_sum - old).max(0.0);
                chain.gpu_known = chain.gpu_known.saturating_sub(1);
            }
        }

        let reported = self.reported()?;
        if reported == pos {
            self.history.push(ft);
            // The session summary counts the frames the lows count: the reported
            // swapchain's, without the gaps.
            if self.collect && ft <= GAP_MS {
                self.session.push(ft);
            }
        }
        let best = &self.chains[reported];
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
/// `pid` is the process this PresentMon follows; it tags what goes to `sessionperf`.
fn parse_stdout(out: impl std::io::Read, pid: u32) {
    let mut reader = BufReader::new(out);
    let mut parser = FrameParser::new();

    // One reused buffer instead of `lines()`, which hands back an owned `String` per
    // line: this reads one line per presented frame, so at the 200-800 fps this path
    // exists for that was 200-800 heap allocations per second, sustained for the
    // whole play session — the most frequent allocation site in the app.
    let mut line = String::new();
    let mut lows_at: u64 = 0;
    let mut graph_version = parser.graph_version();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // EOF.
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let now = crate::metrics::clock_ms();
        // Frames count towards the session summary only while the sampler is drawing
        // over the game in the foreground: one relaxed load per frame.
        parser.set_collect(crate::sessionperf::sampling());
        if let Some((fps, avg_ft)) = parser.feed(line.trim_end(), now) {
            FRAMETIME_X100.store((avg_ft * 100.0) as u32, Ordering::Relaxed);
            FPS_X100.store((fps * 100.0) as u32, Ordering::Relaxed);
            GPU_BUSY_X10.store(
                parser.gpu_busy_pct().map_or(0, |p| (p * 10.0).round() as u32 + 1),
                Ordering::Relaxed,
            );
            LAST_UPDATE_MS.store(now, Ordering::Relaxed);

            // The lows move slowly and cost a copy of the history: twice a second,
            // plus right away when the history restarted under them.
            let lows_due = parser.take_cleared() || now.saturating_sub(lows_at) >= LOWS_EVERY_MS;
            if lows_due {
                lows_at = now;
                let (low_1, low_01) = parser.lows();
                let x100 = |v: Option<f32>| v.map_or(0, |v| (v * 100.0) as u32);
                LOW_1_X100.store(x100(low_1), Ordering::Relaxed);
                LOW_01_X100.store(x100(low_01), Ordering::Relaxed);
            }
            // Session summary: one lock per batch, on the same cadence.
            if lows_due || parser.session_batch_full() {
                parser.drain_session(|frames| crate::sessionperf::add_frames(pid, frames));
            }
            // One lock per finished slice (5 per second), not one per frame.
            if parser.graph_version() != graph_version {
                graph_version = parser.graph_version();
                *GRAPH.lock().unwrap_or_else(PoisonError::into_inner) = parser.graph();
            }
        }
    }
    // Stream ended (game closed / PresentMon stopped): the last half second still
    // belongs to the session, then clear stale numbers.
    parser.drain_session(|frames| crate::sessionperf::add_frames(pid, frames));
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
        log::warn!(
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
        // PresentMon's ETW realtime session needs admin or Performance Log Users.
        // Neither can change while the process runs (group membership is fixed at
        // logon), so check once: without it we never even attempt to spawn it (no
        // access-denied spam, no overhead). AMD still gets fullscreen FPS from the
        // cputemp sidecar regardless.
        let can_trace = {
            #[cfg(windows)]
            {
                crate::elevation::can_trace_etw()
            }
            #[cfg(not(windows))]
            {
                false
            }
        };
        if !can_trace {
            log::info!("PresentMon disabled: not elevated and not in Performance Log Users");
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
            // live, so an idle Astrail does not wake this thread at all.
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
                                log::info!("PresentMon started (pid {}) for game pid {want_pid}", c.id());
                                *child_lock() = Some(c);
                                child_pid = want_pid;
                            }
                            Err(e) => {
                                // Typically "access denied" without elevation. Mark the
                                // PID failed so we don't hammer respawns every tick.
                                log::warn!("PresentMon could not be started: {e}");
                                failed_pid = want_pid;
                            }
                        },
                        None => {
                            // No binary: don't re-scan the filesystem every tick either.
                            failed_pid = want_pid;
                            if !bin_missing_logged {
                                log::warn!(
                                    "PresentMon.exe not found: FPS and frametime disabled"
                                );
                                bin_missing_logged = true;
                            }
                        }
                    }
                }
            }

            // Reap a child that exited on its own (game closed, ETW denied, …).
            let exit_status = match child_lock().as_mut().map(|c| c.try_wait()) {
                Some(Ok(Some(status))) => Some(status),
                _ => None,
            };
            if let Some(status) = exit_status {
                let dead = child_pid;
                log::info!("PresentMon for game pid {dead} exited on its own ({status})");
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

    /// Real PresentMon 2.4.1 output, captured on 2026-09-21 without admin (Performance
    /// Log Users) with the exact arguments `spawn` passes, from DWM on a two-monitor
    /// desktop: two swapchains presenting at different rates, `msGPUActive` filled.
    const REAL_CAPTURE: &str = include_str!("../tests/fixtures/presentmon-dwm-2swapchains.csv");

    /// One data row of the capture, read without `FrameParser` (the oracle).
    struct RealRow {
        chain: u64,
        t_ms: u64,
        ft: f32,
        gpu: f32,
    }

    fn real_rows() -> Vec<RealRow> {
        let mut lines = REAL_CAPTURE.lines();
        let header: Vec<&str> = lines.next().expect("header").split(',').collect();
        let col = |name: &str| header.iter().position(|c| *c == name).expect(name);
        let (sc, t, ft, gpu) = (col("SwapChainAddress"), col("TimeInSeconds"), col("msBetweenPresents"), col("msGPUActive"));
        lines
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let f: Vec<&str> = l.split(',').collect();
                RealRow {
                    chain: parse_address(f[sc]),
                    t_ms: (f[t].parse::<f64>().expect("time") * 1000.0) as u64,
                    ft: f[ft].parse().expect("frametime"),
                    gpu: f[gpu].parse().expect("gpu"),
                }
            })
            .collect()
    }

    /// Frames of one chain inside the parser's 1 s window: the newest ones, dropping
    /// from the front while the sum exceeds `WINDOW_MS` (at least one kept).
    fn last_window(frames: &[f32]) -> &[f32] {
        let mut start = 0;
        let mut sum: f32 = frames.iter().sum();
        while sum > WINDOW_MS && frames.len() - start > 1 {
            sum -= frames[start];
            start += 1;
        }
        &frames[start..]
    }

    #[test]
    fn a_real_capture_replays_to_the_busiest_swapchain() {
        let rows = real_rows();
        assert!(rows.len() > 500, "the capture holds {} rows", rows.len());

        // Replayed on the capture's own clock, as the reader would have seen it live.
        let mut p = FrameParser::new();
        let mut lines = REAL_CAPTURE.lines().filter(|l| !l.trim().is_empty());
        assert_eq!(p.feed(lines.next().expect("header"), 0), None);
        let mut last = None;
        for (line, r) in lines.zip(&rows) {
            if let Some(reading) = p.feed(line, r.t_ms) {
                last = Some(reading);
            }
        }
        let (fps, ft) = last.expect("the capture produces a reading");

        // Oracle: the chain with more presents in its final second is the one shown,
        // at exactly the rate of its own frames, not a blend of both chains.
        let per_chain = |id: u64| rows.iter().filter(|r| r.chain == id).map(|r| r.ft).collect::<Vec<_>>();
        let mut ids: Vec<u64> = rows.iter().map(|r| r.chain).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 2, "the capture was taken on two monitors");
        let busiest = *ids.iter().max_by_key(|&&id| last_window(&per_chain(id)).len()).expect("chains");
        let own = per_chain(busiest);
        let window = last_window(&own);
        let want_ft = window.iter().sum::<f32>() / window.len() as f32;
        assert!((ft - want_ft).abs() < 1e-3, "frametime {ft} vs {want_ft}");
        assert!((fps - 1000.0 / want_ft).abs() < 1e-2, "fps {fps}");
        let all: Vec<f32> = rows.iter().map(|r| r.ft).collect();
        let blended = last_window(&all);
        let blended_fps = 1000.0 * blended.len() as f32 / blended.iter().sum::<f32>();
        assert!((fps - blended_fps).abs() > 1.0, "{fps} would also be the blended rate {blended_fps}");

        // GPU busy over the same window, from the column the capture carries.
        let gpu: Vec<f32> = rows.iter().filter(|r| r.chain == busiest).map(|r| r.gpu).collect();
        let gpu_window = &gpu[gpu.len() - window.len()..];
        let want_busy = (gpu_window.iter().sum::<f32>() / window.iter().sum::<f32>() * 100.0).min(100.0);
        let busy = p.gpu_busy_pct().expect("msGPUActive is present and non-zero");
        assert!((busy - want_busy).abs() < 0.05, "gpu busy {busy} vs {want_busy}");

        // The history holds the reported chain's frames, in order, and nothing else.
        let history: Vec<f32> = p.history.frames.iter().copied().collect();
        let own_no_gaps: Vec<f32> = own.iter().copied().filter(|f| *f <= GAP_MS).collect();
        assert!(!history.is_empty() && history.len() <= own_no_gaps.len());
        assert_eq!(history, own_no_gaps[own_no_gaps.len() - history.len()..]);
        let mut expected = history.clone();
        let (low1, low01) = p.lows();
        assert_eq!((low1, low01), percentile_lows(&mut expected));
        // Under 1000 frames there is no 0.1 % low, never a guess.
        assert!(history.len() < MIN_FRAMES_LOW_01 && low01.is_none());
        if history.len() >= MIN_FRAMES_LOW_1 {
            let low1 = low1.expect("enough frames for a 1 % low");
            assert!(low1 > 0.0 && low1 <= fps, "1% low {low1} above the average {fps}");
        }

        // Graph slices never report a frame longer than the worst one fed.
        let worst = own_no_gaps.iter().copied().fold(0.0f32, f32::max);
        assert!(p.graph().points().iter().all(|v| *v >= 0.0 && *v <= worst + 1e-3));
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

    /// A parser past its header, fed `frames` from one swapchain on a clock that
    /// advances with the frametimes.
    fn fed(frames: impl IntoIterator<Item = f32>) -> FrameParser {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        let mut t = 1.0f64;
        for ft in frames {
            t += f64::from(ft);
            p.feed(&row("0x1", ft), t as u64);
        }
        p
    }

    fn close(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 0.05)
    }

    const FT_60: f32 = 1000.0 / 60.0;

    #[test]
    fn lows_are_the_99th_and_999th_frametime_percentiles() {
        // 990 frames at 60 fps, nine at 40 ms, one at 100 ms: 99 % of the frames beat
        // 40 ms (25 fps) and 99.9 % beat 100 ms (10 fps).
        let mut frames = vec![FT_60; 990];
        frames.extend([40.0; 9]);
        frames.push(100.0);
        // Order must not matter.
        frames.swap(0, 999);
        frames.swap(500, 995);
        let (low_1, low_01) = fed(frames).lows();
        assert!(close(low_1, 25.0), "1% low {low_1:?}");
        assert!(close(low_01, 10.0), "0.1% low {low_01:?}");
    }

    #[test]
    fn a_steady_game_has_lows_equal_to_its_fps() {
        let (low_1, low_01) = fed(vec![FT_60; 1200]).lows();
        assert!(close(low_1, 60.0) && close(low_01, 60.0), "{low_1:?} {low_01:?}");
    }

    #[test]
    fn lows_stay_empty_until_there_are_enough_frames() {
        assert_eq!(fed(vec![FT_60; MIN_FRAMES_LOW_1 - 1]).lows(), (None, None));
        let (low_1, low_01) = fed(vec![FT_60; MIN_FRAMES_LOW_01 - 1]).lows();
        assert!(close(low_1, 60.0));
        assert_eq!(low_01, None);
        assert_eq!(percentile_lows(&mut []), (None, None));
    }

    #[test]
    fn session_frames_are_the_reported_swapchain_without_gaps_and_only_when_asked() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        let mut t = 1u64;
        let mut feed = |p: &mut FrameParser, chain: &str, ft: f32| {
            t += ft as u64;
            p.feed(&row(chain, ft), t);
        };

        // Gate closed (game in the background): nothing is kept.
        for _ in 0..30 {
            feed(&mut p, "0x1", FT_60);
        }
        p.drain_session(|_| panic!("collected with the gate closed"));

        // Gate open: the game's frames, not the 5 fps UI swapchain's, not the gap.
        p.set_collect(true);
        for i in 0..60 {
            feed(&mut p, "0x1", FT_60);
            if i % 12 == 0 {
                feed(&mut p, "0x2", 200.0);
            }
        }
        feed(&mut p, "0x1", 4000.0);
        let mut got = Vec::new();
        p.drain_session(|frames| got.extend_from_slice(frames));
        assert_eq!(got.len(), 60);
        assert!(got.iter().all(|ft| (*ft - FT_60).abs() < 0.01));

        // Drained means handed over once.
        p.drain_session(|_| panic!("the batch was not cleared"));
    }

    #[test]
    fn the_session_batch_never_reallocates() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        p.set_collect(true);
        let capacity = p.session.capacity();
        let mut handed = 0usize;
        for i in 0..5000u64 {
            p.feed(&row("0x1", 2.0), 1 + i * 2);
            // What the reader does after every row.
            if p.session_batch_full() {
                p.drain_session(|frames| handed += frames.len());
            }
        }
        p.drain_session(|frames| handed += frames.len());
        assert_eq!(handed, 5000);
        assert_eq!(p.session.capacity(), capacity);
    }

    #[test]
    fn the_history_is_bounded_and_never_reallocates() {
        // 1000 fps: the 30 s window would be 30 000 frames, the cap is what binds.
        let mut p = fed(vec![1.0; 20_000]);
        assert_eq!(p.history.frames.len(), HISTORY_CAP);
        assert_eq!(p.history.frames.capacity(), FrameParser::new().history.frames.capacity());
        p.lows();
        assert_eq!(p.scratch.capacity(), FrameParser::new().scratch.capacity());

        // 20 fps: the window binds, and old frames leave the lows with it.
        let mut frames = vec![400.0; 5];
        frames.extend(vec![50.0; 1200]);
        let mut p = fed(frames);
        assert!(p.history.total_ms <= HISTORY_WINDOW_MS);
        assert_eq!(p.history.frames.len(), 600);
        assert!(close(p.lows().0, 20.0));
    }

    #[test]
    fn a_loading_gap_is_not_a_slow_frame() {
        let mut frames = vec![FT_60; 1100];
        frames.insert(600, 4000.0);
        let mut p = fed(frames);
        let (low_1, low_01) = p.lows();
        assert!(close(low_1, 60.0) && close(low_01, 60.0), "{low_1:?} {low_01:?}");
        assert!(p.graph().points().iter().all(|ft| (*ft - FT_60).abs() < 0.01));
    }

    #[test]
    fn swapchains_at_the_same_rate_do_not_restart_the_history() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        for i in 0..600u64 {
            let t = 1 + i * 16;
            p.feed(&row("0xA", FT_60), t);
            p.feed(&row("0xB", FT_60), t);
        }
        assert_eq!(p.owner, Some(0xA));
        assert_eq!(p.history.frames.len(), 600);
    }

    #[test]
    fn a_new_busiest_swapchain_starts_a_fresh_history() {
        // A 30 fps menu swapchain, then the 144 fps game one takes over.
        let mut p = fed(vec![1000.0 / 30.0; 300]);
        assert!(p.take_cleared(), "the first owner starts from an empty history");
        assert!(p.lows().0.is_some());
        let version = p.graph_version();
        let mut t = 10_001.0f64;
        for _ in 0..90 {
            t += 1000.0 / 144.0;
            p.feed(&row("0x2", 1000.0 / 144.0), t as u64);
        }
        assert_eq!(p.owner, Some(0x2));
        assert!(p.take_cleared());
        assert_ne!(p.graph_version(), version);
        assert_eq!(p.lows(), (None, None), "menu frames must not count as game frames");
        assert!(p.history.frames.iter().all(|ft| *ft < 10.0));
    }

    #[test]
    fn the_graph_keeps_the_worst_frame_of_each_slice() {
        // 11 frames at 60 fps = 183 ms, then a 30 ms frame that crosses into the next
        // slice, where it ended: slice 1 is clean, slice 2 shows the spike.
        let mut frames = vec![FT_60; 11];
        frames.push(30.0);
        frames.extend(vec![FT_60; 24]);
        let p = fed(frames);
        let points = p.graph().points().to_vec();
        assert_eq!(points.len(), 3, "{points:?}");
        assert!((points[0] - FT_60).abs() < 0.01, "{points:?}");
        assert!((points[1] - 30.0).abs() < 0.01, "{points:?}");
        assert!((points[2] - FT_60).abs() < 0.01, "{points:?}");
    }

    #[test]
    fn a_long_frame_fills_the_slices_it_spans() {
        // 50 ms of normal frames, then a 500 ms hitch (50..550 ms): nothing ended in
        // 200..400, so that slice is the hitch; it ended in 400..600.
        let mut frames = vec![FT_60; 3];
        frames.push(500.0);
        frames.extend(vec![FT_60; 5]);
        let p = fed(frames);
        let points = p.graph().points().to_vec();
        assert_eq!(points.len(), 3, "{points:?}");
        assert!((points[0] - FT_60).abs() < 0.01, "{points:?}");
        assert_eq!(&points[1..], &[500.0, 500.0]);
    }

    #[test]
    fn the_graph_holds_the_last_sixty_slices() {
        // 100 slices of exactly 200 ms at 100 fps; the last ten carry a 25 ms frame.
        let mut frames = Vec::new();
        for slice in 0..100 {
            if slice < 90 {
                frames.extend([10.0; 20]);
            } else {
                frames.extend([10.0; 8]);
                frames.push(25.0);
                frames.extend([10.0; 9]);
                frames.push(5.0);
            }
        }
        let graph = fed(frames).graph();
        assert_eq!(graph.points().len(), GRAPH_POINTS);
        assert_eq!(graph.points().iter().filter(|ft| **ft == 25.0).count(), 10);
        assert_eq!(graph.points()[GRAPH_POINTS - 1], 25.0);
        assert_eq!(graph.points()[0], 10.0);
    }

    #[test]
    fn a_graph_serializes_as_its_points() {
        let json = serde_json::to_string(&FrameGraph::from_points(&[16.5, 33.0])).expect("json");
        assert_eq!(json, "[16.5,33.0]");
    }

    /// The same stream with GPU tracking on: the parser finds columns by name, so
    /// only the two names matter here, not their position.
    const GPU_HEADER: &str = "Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,Dropped,TimeInSeconds,msInPresentAPI,msBetweenPresents,msUntilRenderStart,msGPUActive";

    fn gpu_row(chain: &str, ft: f32, gpu: &str) -> String {
        format!("game.exe,1234,{chain},DXGI,0,0,0,1.0,0.1,{ft},0.5,{gpu}")
    }

    /// `count` frames of 10 ms on one swapchain, each with the same GPU field.
    fn gpu_feed(p: &mut FrameParser, start_ms: u64, count: u64, gpu: &str) -> u64 {
        for i in 0..count {
            p.feed(&gpu_row("0x00000001", 10.0, gpu), start_ms + i * 10);
        }
        start_ms + count * 10
    }

    #[test]
    fn gpu_busy_is_the_gpu_share_of_the_frame_time() {
        let mut p = FrameParser::new();
        p.feed(GPU_HEADER, 1);
        gpu_feed(&mut p, 1, 150, "9.3");
        let busy = p.gpu_busy_pct().expect("a full window of readings");
        assert!((busy - 93.0).abs() < 0.1, "busy {busy}");
    }

    #[test]
    fn without_the_gpu_column_nothing_is_reported() {
        let mut p = FrameParser::new();
        p.feed(HEADER, 1);
        for i in 0..150 {
            p.feed(&row("0x00000001", 10.0), 1 + i * 10);
        }
        assert_eq!(p.gpu_busy_pct(), None);
    }

    #[test]
    fn an_all_zero_gpu_column_is_unknown_not_idle() {
        // A PresentMon that cannot see GPU events still writes the column, as zeros.
        let mut p = FrameParser::new();
        p.feed(GPU_HEADER, 1);
        gpu_feed(&mut p, 1, 150, "0.000");
        assert_eq!(p.gpu_busy_pct(), None);
    }

    #[test]
    fn a_missing_gpu_reading_hides_the_share_until_it_leaves_the_window() {
        let mut p = FrameParser::new();
        p.feed(GPU_HEADER, 1);
        let t = gpu_feed(&mut p, 1, 100, "5.0");
        assert!(p.gpu_busy_pct().is_some());
        let t = gpu_feed(&mut p, t, 1, "NA");
        let t = gpu_feed(&mut p, t, 50, "5.0");
        assert_eq!(p.gpu_busy_pct(), None, "a partial sum would read low");
        gpu_feed(&mut p, t, 120, "5.0");
        let busy = p.gpu_busy_pct().expect("the gap rolled out of the window");
        assert!((busy - 50.0).abs() < 0.1, "busy {busy}");
    }

    #[test]
    fn gpu_busy_waits_for_half_a_window() {
        let mut p = FrameParser::new();
        p.feed(GPU_HEADER, 1);
        let t = gpu_feed(&mut p, 1, 49, "8.0");
        assert_eq!(p.gpu_busy_pct(), None, "490 ms of frames");
        gpu_feed(&mut p, t, 1, "8.0");
        assert!(p.gpu_busy_pct().is_some(), "500 ms of frames");

        // Enough time, too few frames: a slideshow says nothing about the GPU.
        let mut slow = FrameParser::new();
        slow.feed(GPU_HEADER, 1);
        for i in 0..5 {
            slow.feed(&gpu_row("0x00000001", 190.0, "100.0"), 1 + i * 190);
        }
        assert_eq!(slow.gpu_busy_pct(), None);
    }

    #[test]
    fn overlapping_gpu_work_is_capped_at_100() {
        let mut p = FrameParser::new();
        p.feed(GPU_HEADER, 1);
        gpu_feed(&mut p, 1, 150, "14.0");
        assert_eq!(p.gpu_busy_pct(), Some(100.0));
    }

    #[test]
    fn gpu_busy_follows_the_reported_swapchain() {
        // The 30 fps embedded UI barely uses the GPU; the share shown next to the
        // game's FPS must be the game's.
        let mut p = FrameParser::new();
        p.feed(GPU_HEADER, 1);
        let (ft_game, ft_ui) = (1000.0f32 / 144.0, 1000.0f32 / 30.0);
        let (mut t_game, mut t_ui) = (0.0f32, 0.0f32);
        while t_game < 3000.0 {
            if t_ui <= t_game {
                p.feed(&gpu_row("0x000002AA", ft_ui, "1.0"), 1 + t_ui as u64);
                t_ui += ft_ui;
            } else {
                let gpu = format!("{}", ft_game * 0.9);
                p.feed(&gpu_row("0x000001BB", ft_game, &gpu), 1 + t_game as u64);
                t_game += ft_game;
            }
        }
        let busy = p.gpu_busy_pct().expect("the game swapchain has readings");
        assert!((busy - 90.0).abs() < 0.5, "busy {busy}");
    }

    #[test]
    fn nothing_is_reported_without_a_fresh_frame() {
        // The only test that touches the published statics.
        LOW_1_X100.store(5_000, Ordering::Relaxed);
        GPU_BUSY_X10.store(931, Ordering::Relaxed);
        *GRAPH.lock().unwrap_or_else(PoisonError::into_inner) = FrameGraph::from_points(&[10.0, 12.0]);
        reset();
        assert_eq!(current(), FrameStats::default());
        assert_eq!(graph(), None);
    }

    #[test]
    fn swapchain_addresses_parse_with_or_without_prefix() {
        assert_eq!(parse_address(" 0x0000020F3A1B2C40 "), 0x0000_020F_3A1B_2C40);
        assert_eq!(parse_address("FF"), 0xFF);
        assert_eq!(parse_address("n/a"), 0);
    }
}
