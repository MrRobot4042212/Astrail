// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Performance summary of a play session: average FPS, 1 % low and peak
//! temperatures, stored next to the session in `playtime.json`.
//!
//! Threads (nothing here blocks, every lock is held for a few hundred
//! nanoseconds to a few microseconds):
//! - the **watcher** (`playtime.rs`) names the session being measured with
//!   [`set_target`] on every poll and collects the result with [`take`] when the
//!   session ends;
//! - the **PresentMon reader** (`presentmon.rs`) hands over frametimes in batches
//!   with [`add_frames`], twice a second, never per frame;
//! - the **sampler** (`metrics.rs`) opens and closes the gate with
//!   [`set_sampling`] and reports temperatures with [`record_temps`], once per
//!   drawn tick.
//!
//! What a summary covers, and what it does not:
//! - Only what the HUD measured: frames presented while the game was the foreground
//!   window with the overlay on. Alt-tabbed time is left out by the gate; menus and
//!   loading screens are in, because nothing tells them apart from play.
//! - FPS figures come from PresentMon's per-frame data alone. The sidecar's integer,
//!   system-wide FPS is never folded in: an average of rounded one-second readings
//!   next to a measured one would look just as exact.
//! - The 1 % low is read from a log-scale histogram (1 % wide buckets), so it is
//!   within ±0.5 % of the exact percentile `presentmon::percentile_lows` would give
//!   over the same frames, at a constant ~3 KB per session instead of every frame.
//! - No 0.1 % low: over a whole session it is decided by a handful of hitches
//!   (shader compilation, a level load), not by how the game ran.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

/// Frame time behind the FPS figures below which they are not reported: a session
/// that barely reached gameplay has no meaningful average.
const MIN_MEASURED_MS: f64 = 30_000.0;
/// Fewer frames than this and the 1 % low is less than one frame wide.
const MIN_FRAMES_LOW_1: u64 = 100;
/// Histogram range and resolution. 0.5 ms is 2000 fps; anything slower than
/// `HIST_MAX_MS` is a gap and never gets here (`presentmon::GAP_MS`).
const HIST_MIN_MS: f64 = 0.5;
const HIST_MAX_MS: f64 = 1000.0;
const HIST_RATIO: f64 = 1.01;
/// `ceil(ln(HIST_MAX_MS / HIST_MIN_MS) / ln(HIST_RATIO))`, checked by a test.
const HIST_BUCKETS: usize = 764;
/// Readings outside this range are a sensor glitch, not a temperature.
const TEMP_MAX_C: u32 = 150;

/// What was measured during one play session. Every figure is optional: FPS needs
/// PresentMon (administrator), CPU temperature needs the sensor sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SessionPerf {
    /// Frames presented divided by the time they took, not a mean of FPS readings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_fps: Option<f32>,
    /// FPS of the frametime 99 % of the session's frames beat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_1_fps: Option<f32>,
    /// Seconds of frames behind the two figures above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_gpu_temp_c: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_temp_c: Option<u32>,
}

/// Running totals of one session.
struct Acc {
    id: String,
    pid: u32,
    frames: u64,
    sum_ms: f64,
    hist: Box<[u32; HIST_BUCKETS]>,
    /// 0 = never read.
    max_gpu_temp_c: u32,
    max_cpu_temp_c: u32,
}

impl Acc {
    fn new(id: &str, pid: u32) -> Self {
        Self {
            id: id.to_string(),
            pid,
            frames: 0,
            sum_ms: 0.0,
            hist: Box::new([0; HIST_BUCKETS]),
            max_gpu_temp_c: 0,
            max_cpu_temp_c: 0,
        }
    }

    fn add_frame(&mut self, ft: f32) {
        let ft = f64::from(ft);
        if !ft.is_finite() || ft <= 0.0 || ft > HIST_MAX_MS {
            return;
        }
        self.frames += 1;
        self.sum_ms += ft;
        let slot = &mut self.hist[bucket_of(ft)];
        *slot = slot.saturating_add(1);
    }

    fn add_temps(&mut self, gpu: Option<u32>, cpu: Option<u32>) {
        let plausible = |t: Option<u32>| t.filter(|t| (1..=TEMP_MAX_C).contains(t)).unwrap_or(0);
        self.max_gpu_temp_c = self.max_gpu_temp_c.max(plausible(gpu));
        self.max_cpu_temp_c = self.max_cpu_temp_c.max(plausible(cpu));
    }

    /// FPS of the `frames / 100`-th slowest frame: the same element
    /// `presentmon::percentile_lows` selects, located by bucket.
    fn low_1_fps(&self) -> Option<f32> {
        if self.frames < MIN_FRAMES_LOW_1 {
            return None;
        }
        let worst = self.frames / 100;
        let mut seen: u64 = 0;
        for (i, count) in self.hist.iter().enumerate().rev() {
            seen += u64::from(*count);
            if seen >= worst {
                return Some(round_1((1000.0 / bucket_mid_ms(i)) as f32));
            }
        }
        None
    }

    fn finish(&self) -> Option<SessionPerf> {
        let has_fps = self.frames > 0 && self.sum_ms >= MIN_MEASURED_MS;
        let perf = SessionPerf {
            avg_fps: has_fps.then(|| round_1((self.frames as f64 * 1000.0 / self.sum_ms) as f32)),
            low_1_fps: if has_fps { self.low_1_fps() } else { None },
            fps_secs: has_fps.then(|| (self.sum_ms / 1000.0).round() as u32),
            max_gpu_temp_c: (self.max_gpu_temp_c > 0).then_some(self.max_gpu_temp_c),
            max_cpu_temp_c: (self.max_cpu_temp_c > 0).then_some(self.max_cpu_temp_c),
        };
        let empty = perf.avg_fps.is_none()
            && perf.max_gpu_temp_c.is_none()
            && perf.max_cpu_temp_c.is_none();
        (!empty).then_some(perf)
    }
}

fn bucket_of(ft_ms: f64) -> usize {
    let raw = (ft_ms / HIST_MIN_MS).ln() / HIST_RATIO.ln();
    // Faster than 2000 fps lands in the first bucket; NaN cannot get here.
    (raw.max(0.0) as usize).min(HIST_BUCKETS - 1)
}

/// Geometric middle of a bucket, so the error is symmetric in ratio.
fn bucket_mid_ms(index: usize) -> f64 {
    HIST_MIN_MS * HIST_RATIO.powf(index as f64 + 0.5)
}

/// One decimal: what the UI shows, and it keeps `playtime.json` short.
fn round_1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

struct State {
    /// Id of the session frames and temperatures are credited to.
    current: Option<String>,
    /// One entry per session that was ever the target and has not ended. More than
    /// one only while two tracked games run at once.
    sessions: Vec<Acc>,
}

impl State {
    fn current_mut(&mut self) -> Option<&mut Acc> {
        let id = self.current.as_deref()?;
        self.sessions.iter_mut().find(|a| a.id == id)
    }
}

static STATE: Mutex<State> = Mutex::new(State { current: None, sessions: Vec::new() });
/// True while the sampler is drawing the HUD over the game in the foreground.
static SAMPLING: AtomicBool = AtomicBool::new(false);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Name the session being measured (the HUD's game) and the pid PresentMon follows
/// for it. `None` when no game owns the HUD. Called on every watcher poll, so the
/// steady state allocates nothing.
pub fn set_target(id: Option<&str>, pid: Option<u32>) {
    let mut st = state();
    let Some(id) = id else {
        st.current = None;
        return;
    };
    let pid = pid.unwrap_or(0);
    match st.sessions.iter_mut().find(|a| a.id == id) {
        Some(acc) => acc.pid = pid,
        None => st.sessions.push(Acc::new(id, pid)),
    }
    if st.current.as_deref() != Some(id) {
        st.current = Some(id.to_string());
    }
}

/// Open or close the gate. Closed whenever the HUD is not being drawn: game in the
/// background, overlay off, settings screen open, no game.
pub fn set_sampling(on: bool) {
    SAMPLING.store(on, Ordering::Relaxed);
}

/// Whether frames presented right now belong in the summary. Read per frame by the
/// PresentMon reader.
pub fn sampling() -> bool {
    SAMPLING.load(Ordering::Relaxed)
}

/// Credit a batch of frametimes (ms) to the current session. `pid` is the process
/// PresentMon was started for: a batch left over from another process is dropped.
pub fn add_frames(pid: u32, frametimes: &[f32]) {
    if frametimes.is_empty() {
        return;
    }
    let mut st = state();
    let Some(acc) = st.current_mut() else { return };
    if acc.pid == 0 || acc.pid != pid {
        return;
    }
    for ft in frametimes {
        acc.add_frame(*ft);
    }
}

/// Credit one temperature reading to the current session.
pub fn record_temps(gpu_temp_c: Option<u32>, cpu_temp_c: Option<u32>) {
    if gpu_temp_c.is_none() && cpu_temp_c.is_none() {
        return;
    }
    if let Some(acc) = state().current_mut() {
        acc.add_temps(gpu_temp_c, cpu_temp_c);
    }
}

/// Close the books on a session that ended. `None` when nothing was measured. Also
/// called for sessions too short to be recorded, so their entry does not linger.
pub fn take(id: &str) -> Option<SessionPerf> {
    let mut st = state();
    if st.current.as_deref() == Some(id) {
        st.current = None;
    }
    let pos = st.sessions.iter().position(|a| a.id == id)?;
    st.sessions.swap_remove(pos).finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FT_60: f32 = 1000.0 / 60.0;

    fn acc_with(frames: &[(usize, f32)]) -> Acc {
        let mut acc = Acc::new("steam:1", 10);
        for (count, ft) in frames {
            for _ in 0..*count {
                acc.add_frame(*ft);
            }
        }
        acc
    }

    fn within(actual: f32, expected: f32, pct: f32) -> bool {
        (actual - expected).abs() <= expected * pct / 100.0
    }

    #[test]
    fn the_bucket_count_matches_the_range() {
        let needed = ((HIST_MAX_MS / HIST_MIN_MS).ln() / HIST_RATIO.ln()).ceil() as usize;
        assert_eq!(needed, HIST_BUCKETS);
        assert_eq!(bucket_of(HIST_MAX_MS), HIST_BUCKETS - 1);
        assert_eq!(bucket_of(0.01), 0);
    }

    #[test]
    fn every_bucket_middle_is_within_half_a_percent_of_its_frames() {
        for ft in [0.7_f64, 2.3, 6.94, 16.67, 33.3, 120.0, 900.0] {
            let mid = bucket_mid_ms(bucket_of(ft));
            assert!((mid / ft - 1.0).abs() <= 0.0051, "{ft} ms read back as {mid} ms");
        }
    }

    #[test]
    fn average_fps_is_frames_over_time_not_a_mean_of_readings() {
        // 60 s at 60 fps and 60 s at 30 fps: 5400 frames in 120 s is 45 fps. A mean
        // of per-frame FPS would say 50, because the fast half has twice the frames.
        let acc = acc_with(&[(3600, FT_60), (1800, 1000.0 / 30.0)]);
        let perf = acc.finish().expect("measured");
        assert!(within(perf.avg_fps.expect("avg"), 45.0, 0.3), "{perf:?}");
        assert_eq!(perf.fps_secs, Some(120));
    }

    #[test]
    fn the_session_low_matches_the_exact_percentile() {
        // Of 4000 frames, 39 took 40 ms and 1 took 100 ms. The worst 1 % is 40 frames,
        // and the fastest of those is a 40 ms frame: 25 fps.
        let acc = acc_with(&[(3960, FT_60), (39, 40.0), (1, 100.0)]);
        let perf = acc.finish().expect("measured");
        assert!(within(perf.low_1_fps.expect("low"), 25.0, 1.0), "{perf:?}");
    }

    #[test]
    fn a_steady_session_has_a_low_equal_to_its_average() {
        let acc = acc_with(&[(3600, FT_60)]);
        let perf = acc.finish().expect("measured");
        assert!(within(perf.low_1_fps.expect("low"), 60.0, 1.0), "{perf:?}");
        assert!(within(perf.avg_fps.expect("avg"), 60.0, 0.3), "{perf:?}");
    }

    #[test]
    fn under_thirty_seconds_of_frames_report_no_fps() {
        let mut acc = acc_with(&[(1200, FT_60)]); // 20 s
        assert_eq!(acc.finish(), None);
        // Temperatures do not need frames.
        acc.add_temps(Some(71), None);
        let perf = acc.finish().expect("temps only");
        assert_eq!(perf.avg_fps, None);
        assert_eq!(perf.fps_secs, None);
        assert_eq!(perf.max_gpu_temp_c, Some(71));
    }

    #[test]
    fn gaps_and_garbage_are_not_frames() {
        let mut acc = acc_with(&[(10, FT_60)]);
        for ft in [0.0, -4.0, f32::NAN, f32::INFINITY, 1000.1, 25_000.0] {
            acc.add_frame(ft);
        }
        assert_eq!(acc.frames, 10);
    }

    #[test]
    fn peak_temperatures_ignore_implausible_readings() {
        let mut acc = Acc::new("steam:1", 10);
        acc.add_temps(Some(64), Some(0));
        acc.add_temps(Some(83), Some(255));
        acc.add_temps(Some(79), Some(72));
        acc.add_temps(None, Some(68));
        let perf = acc.finish().expect("temps");
        assert_eq!(perf.max_gpu_temp_c, Some(83));
        assert_eq!(perf.max_cpu_temp_c, Some(72));
    }

    #[test]
    fn an_empty_summary_serializes_to_nothing_and_old_files_still_load() {
        let perf = SessionPerf {
            avg_fps: Some(143.3),
            low_1_fps: None,
            fps_secs: Some(1800),
            max_gpu_temp_c: Some(74),
            max_cpu_temp_c: None,
        };
        let json = serde_json::to_string(&perf).expect("serialize");
        assert_eq!(json, r#"{"avg_fps":143.3,"fps_secs":1800,"max_gpu_temp_c":74}"#);
        let back: SessionPerf = serde_json::from_str("{}").expect("all fields optional");
        assert_eq!(back.avg_fps, None);
    }

    /// The statics are process-wide, so everything that goes through them lives in
    /// this one test, under ids no other test uses.
    #[test]
    fn frames_are_credited_to_the_target_session_only() {
        let frames = vec![FT_60; 2400]; // 40 s

        // No target: dropped.
        add_frames(77, &frames);
        assert_eq!(take("test:a"), None);

        set_target(Some("test:a"), Some(77));
        add_frames(78, &frames); // another process's leftovers
        add_frames(77, &frames);
        record_temps(Some(70), Some(61));

        // A second game takes the HUD: the first keeps what it has.
        set_target(Some("test:b"), Some(90));
        add_frames(77, &frames);
        add_frames(90, &frames[..600]); // 10 s: not enough for FPS
        record_temps(Some(55), None);

        let a = take("test:a").expect("a was measured");
        assert_eq!(a.fps_secs, Some(40));
        assert_eq!(a.max_gpu_temp_c, Some(70));
        assert_eq!(a.max_cpu_temp_c, Some(61));

        let b = take("test:b").expect("b has a temperature");
        assert_eq!(b.avg_fps, None);
        assert_eq!(b.max_gpu_temp_c, Some(55));

        // Taking the target clears it, and a taken session is gone.
        add_frames(90, &frames);
        assert_eq!(take("test:b"), None);

        // A pid that was never resolved (0) measures nothing.
        set_target(Some("test:c"), None);
        add_frames(0, &frames);
        assert_eq!(take("test:c"), None);
    }
}
