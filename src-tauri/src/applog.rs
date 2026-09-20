// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! File logging and crash reports.
//!
//! A release build is a `windows_subsystem = "windows"` process, so stderr goes
//! nowhere and, with `panic = "abort"`, a panic on any thread ends the app without
//! a trace. This module is the `log` backend for the whole crate: one size-capped
//! file in the app log dir, plus a panic hook that leaves a `crash-<ts>.txt` behind
//! before the abort.
//!
//! It is deliberately not `tauri-plugin-log`: the logger has to exist before the
//! Tauri builder does (the single-instance hand-off and a failed `build()` both
//! happen first), the panic hook needs the directory without an `AppHandle`, and
//! the frontend does not get a logging capability it has no use for.
//!
//! Thread ownership: any thread may log. A record is one `write_all` on an
//! unbuffered `File` under a mutex, so a line is on disk when the macro returns
//! and a crash right after it does not lose it. Nothing here may be called per
//! sampler tick; log state changes, not samples.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

pub const LOG_FILE: &str = "astrail.log";
const ROTATED_FILE: &str = "astrail.log.1";
/// The live file is rotated once it passes this, and one rotated copy is kept, so
/// the logs never take more than twice this on disk.
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const CRASH_PREFIX: &str = "crash-";
const MAX_CRASH_FILES: usize = 5;
/// Records from this crate. Everything else (tao, wry, ureq, rustls…) is only
/// interesting when it warns.
const OWN_TARGET: &str = "astrail";

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static LOGGER: FileLogger = FileLogger { sink: Mutex::new(None) };

struct Sink {
    file: File,
    written: u64,
}

struct FileLogger {
    sink: Mutex<Option<Sink>>,
}

/// Same directory Tauri's `app_log_dir()` resolves to on Windows, computed without
/// an `AppHandle` so it is available before the builder runs.
pub fn default_dir(identifier: &str) -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(base).join(identifier).join("logs"))
}

/// Install the file logger and the panic hook. Safe to call once; a failure to
/// open the file leaves logging off rather than failing startup.
pub fn init(dir: PathBuf) {
    if LOG_DIR.set(dir.clone()).is_err() {
        return;
    }
    let _ = fs::create_dir_all(&dir);
    *LOGGER.sink.lock().unwrap_or_else(PoisonError::into_inner) = open_sink(&dir);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(max_level());
    }
    install_panic_hook();
    prune_crash_files(&dir);
}

fn max_level() -> LevelFilter {
    match std::env::var("ASTRAIL_LOG").ok().as_deref() {
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}

fn open_sink(dir: &Path) -> Option<Sink> {
    let path = dir.join(LOG_FILE);
    let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    let written = file.metadata().map(|m| m.len()).unwrap_or(0);
    Some(Sink { file, written })
}

/// Move the live file to the single rotated slot and start a new one.
fn rotate(dir: &Path) -> Option<Sink> {
    let _ = fs::remove_file(dir.join(ROTATED_FILE));
    let _ = fs::rename(dir.join(LOG_FILE), dir.join(ROTATED_FILE));
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join(LOG_FILE))
        .ok()?;
    Some(Sink { file, written: 0 })
}

/// Whether a record is kept: ours at the configured level, dependencies only
/// from `Warn` up.
fn wanted(target: &str, level: Level) -> bool {
    target.starts_with(OWN_TARGET) || level <= Level::Warn
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        wanted(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}\n",
            timestamp(now_ms()),
            record.level(),
            record.target(),
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        let mut guard = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_some_and(|s| s.written >= MAX_LOG_BYTES) {
            if let Some(dir) = LOG_DIR.get() {
                // Drop the old handle first: Windows will not rename an open file.
                *guard = None;
                *guard = rotate(dir);
            }
        }
        if let Some(sink) = guard.as_mut() {
            if sink.file.write_all(line.as_bytes()).is_ok() {
                sink.written += line.len() as u64;
            }
        }
    }

    fn flush(&self) {}
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

/// `2026-09-18T14:03:07.123Z` from milliseconds since the Unix epoch. UTC on
/// purpose: a log shared across time zones has to line up with the reporter's
/// description without guessing an offset.
pub fn timestamp(ms: u128) -> String {
    let secs = (ms / 1000) as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        ms % 1000
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let report = crash_report(
            thread.name().unwrap_or("<unnamed>"),
            &info.to_string(),
            &std::backtrace::Backtrace::force_capture().to_string(),
        );
        // The crash file first: it takes no lock, so it is written even if the
        // panic came from inside the logger.
        if let Some(dir) = LOG_DIR.get() {
            let _ = fs::write(dir.join(format!("{CRASH_PREFIX}{}.txt", now_ms() / 1000)), report);
        }
        if let Ok(mut guard) = LOGGER.sink.try_lock() {
            if let Some(sink) = guard.as_mut() {
                let _ = writeln!(sink.file, "{} ERROR astrail::panic: {info}", timestamp(now_ms()));
            }
        }
        previous(info);
    }));
}

/// Body of a crash file. The release binary is stripped, so the backtrace is
/// addresses only; the panic message carries the `file:line`, which is the part
/// that identifies the bug.
fn crash_report(thread: &str, message: &str, backtrace: &str) -> String {
    format!(
        "Astrail {} crashed\ntime: {}\nthread: {thread}\nelevated: {}\n\n{message}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        timestamp(now_ms()),
        is_elevated(),
    )
}

fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        crate::elevation::is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Crash files in `dir`, oldest first (the name embeds the Unix time).
fn crash_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(CRASH_PREFIX) && n.ends_with(".txt"))
        })
        .collect();
    files.sort();
    files
}

fn prune_crash_files(dir: &Path) {
    let files = crash_files(dir);
    let excess = files.len().saturating_sub(MAX_CRASH_FILES);
    for old in &files[..excess] {
        let _ = fs::remove_file(old);
    }
}

/// Last `max_bytes` of a text file, cut at a line boundary. Missing file = empty.
fn tail(path: &Path, max_bytes: usize) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    let start = bytes.len().saturating_sub(max_bytes);
    let slice = &bytes[start..];
    let slice = match (start, slice.iter().position(|&b| b == b'\n')) {
        (0, _) | (_, None) => slice,
        (_, Some(nl)) => &slice[nl + 1..],
    };
    String::from_utf8_lossy(slice).into_owned()
}

/// Write `astrail-diagnostics-<ts>.txt` next to the logs: the caller's header
/// (version, hardware, settings), every crash report and the end of both log
/// files. One plain text file so it can be read before it is shared.
pub fn write_diagnostics(header: &str) -> Result<PathBuf, String> {
    let dir = LOG_DIR.get().ok_or("logging is not initialised")?;
    const LOG_TAIL_BYTES: usize = 256 * 1024;
    let mut out = String::with_capacity(LOG_TAIL_BYTES);
    out.push_str(header);
    for crash in crash_files(dir) {
        out.push_str(&format!("\n===== {} =====\n", crash.file_name().unwrap_or_default().to_string_lossy()));
        out.push_str(&tail(&crash, 32 * 1024));
    }
    for name in [ROTATED_FILE, LOG_FILE] {
        out.push_str(&format!("\n===== {name} =====\n"));
        out.push_str(&tail(&dir.join(name), LOG_TAIL_BYTES));
    }
    let path = dir.join(format!("astrail-diagnostics-{}.txt", now_ms() / 1000));
    fs::write(&path, out).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Message from the webview, reduced to something that cannot flood or forge the
/// log: one line, bounded length.
pub fn sanitize_frontend_message(message: &str) -> String {
    const MAX_CHARS: usize = 2000;
    let flat: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_CHARS)
        .collect();
    flat.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_iso_8601() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        // 2026-09-18 14:03:07.123 UTC.
        assert_eq!(timestamp(1_789_740_187_123), "2026-09-18T14:03:07.123Z");
        // Leap day and the last second of a year.
        assert_eq!(timestamp(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
        assert_eq!(timestamp(1_735_689_599_999), "2024-12-31T23:59:59.999Z");
    }

    #[test]
    fn dependencies_only_get_through_when_they_warn() {
        assert!(wanted("astrail_lib::presentmon", Level::Info));
        assert!(wanted("astrail_lib::presentmon", Level::Debug));
        assert!(!wanted("ureq::unit", Level::Info));
        assert!(wanted("ureq::unit", Level::Warn));
        assert!(wanted("rustls::conn", Level::Error));
    }

    #[test]
    fn rotation_keeps_one_previous_file() {
        let dir = std::env::temp_dir().join(format!("astrail-applog-{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(LOG_FILE), "first\n").unwrap();
        fs::write(dir.join(ROTATED_FILE), "stale\n").unwrap();
        let sink = rotate(&dir).expect("a new live file");
        assert_eq!(sink.written, 0);
        assert_eq!(fs::read_to_string(dir.join(ROTATED_FILE)).unwrap(), "first\n");
        assert_eq!(fs::read_to_string(dir.join(LOG_FILE)).unwrap(), "");
        drop(sink);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_crash_reports_are_kept() {
        let dir = std::env::temp_dir().join(format!("astrail-crash-{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        for ts in 100..108 {
            fs::write(dir.join(format!("{CRASH_PREFIX}{ts}.txt")), "x").unwrap();
        }
        fs::write(dir.join("astrail.log"), "not a crash file").unwrap();
        prune_crash_files(&dir);
        let left = crash_files(&dir);
        assert_eq!(left.len(), MAX_CRASH_FILES);
        assert!(left[0].ends_with("crash-103.txt"), "{left:?}");
        assert!(dir.join("astrail.log").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tail_starts_on_a_line_boundary() {
        let dir = std::env::temp_dir().join(format!("astrail-tail-{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.log");
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(tail(&path, 9), "three\n");
        assert_eq!(tail(&path, 1000), "one\ntwo\nthree\n");
        assert_eq!(tail(&dir.join("missing.log"), 10), "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_frontend_message_cannot_forge_a_second_log_line() {
        let got = sanitize_frontend_message("boom\n2026-01-01T00:00:00.000Z ERROR astrail: fake\r\n");
        assert!(!got.contains('\n') && !got.contains('\r'));
        assert_eq!(sanitize_frontend_message(&"x".repeat(5000)).len(), 2000);
    }

    #[test]
    fn a_crash_report_names_the_version_thread_and_message() {
        let report = crash_report("metrics-sampler", "panicked at src/metrics.rs:10:5: boom", "0: 0x1");
        assert!(report.contains(env!("CARGO_PKG_VERSION")));
        assert!(report.contains("thread: metrics-sampler"));
        assert!(report.contains("src/metrics.rs:10:5"));
    }
}
