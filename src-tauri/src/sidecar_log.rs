// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Sidecar stderr → `astrail.log`.
//!
//! PresentMon and `cputemp.exe` explain their failures on stderr ("failed to
//! start trace session", "cputemp: open failed: …" when the sensor driver is
//! missing or blocked). That stream used to go to `Stdio::null()`, so the log
//! said a sidecar exited and never why. Each child now gets one reader thread
//! that forwards a bounded number of lines at `warn`; the rest is counted, so a
//! sidecar that spams cannot fill the 1 MiB log.
//!
//! A sidecar may also declare a `Notice`: text it prints on every start that is
//! expected and harmless (PresentMon without elevation explains, on four lines,
//! that it cannot see processes of other accounts). Those lines are summed up in
//! one `info` line per run instead of four warnings on every game.

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};

/// Expected stderr text of a sidecar, logged once per run as `summary`.
pub struct Notice {
    /// Fragments that identify the notice's lines; a line containing any of them
    /// belongs to it.
    pub fragments: &'static [&'static str],
    pub summary: &'static str,
    pub logged: AtomicBool,
}

impl Notice {
    fn matches(&self, line: &str) -> bool {
        self.fragments.iter().any(|f| line.contains(f))
    }
}

/// Lines forwarded per child; anything after is only counted.
const MAX_LINES: usize = 20;
/// Characters kept per line.
const MAX_CHARS: usize = 300;

/// Forward `stderr` of the sidecar `name` to the log on its own thread. The
/// thread ends when the child closes the pipe (exit or kill).
pub fn forward<R: Read + Send + 'static>(name: &'static str, stderr: R, notice: Option<&'static Notice>) {
    std::thread::spawn(move || {
        let skipped = pump(BufReader::new(stderr), |line| match notice {
            Some(n) if n.matches(line) => {
                if !n.logged.swap(true, Ordering::Relaxed) {
                    log::info!("{name}: {}", n.summary);
                }
            }
            _ => log::warn!("{name} stderr: {line}"),
        });
        if skipped > 0 {
            log::warn!("{name} stderr: {skipped} more lines not logged");
        }
    });
}

/// Hand the first `MAX_LINES` non-empty lines, trimmed and capped to
/// `MAX_CHARS`, to `sink`. Returns how many non-empty lines were left out.
/// Stops at EOF or at the first read error; invalid UTF-8 is replaced, never
/// fatal.
fn pump<R: BufRead>(mut reader: R, mut sink: impl FnMut(&str)) -> usize {
    let mut buf = Vec::new();
    let (mut sent, mut skipped) = (0usize, 0usize);
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return skipped,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        if sent < MAX_LINES {
            sent += 1;
            match line.char_indices().nth(MAX_CHARS) {
                Some((cut, _)) => sink(&format!("{}…", &line[..cut])),
                None => sink(line),
            }
        } else {
            skipped += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[u8]) -> (Vec<String>, usize) {
        let mut out = Vec::new();
        let skipped = pump(input, |l| out.push(l.to_string()));
        (out, skipped)
    }

    #[test]
    fn lines_are_trimmed_and_blank_ones_dropped() {
        let (out, skipped) = run(b"cputemp: open failed: driver missing\r\n\r\n  \nsecond\n");
        assert_eq!(out, vec!["cputemp: open failed: driver missing", "second"]);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn a_chatty_sidecar_is_capped_and_counted() {
        let input: String = (0..MAX_LINES + 7).map(|i| format!("line {i}\n")).collect();
        let (out, skipped) = run(input.as_bytes());
        assert_eq!(out.len(), MAX_LINES);
        assert_eq!(skipped, 7);
    }

    #[test]
    fn long_lines_are_cut_on_a_char_boundary_and_bad_utf8_survives() {
        let long = "é".repeat(MAX_CHARS + 50);
        let mut input = format!("{long}\n").into_bytes();
        input.extend_from_slice(b"\xff\xfe oops\n");
        let (out, _) = run(&input);
        assert_eq!(out[0].chars().count(), MAX_CHARS + 1);
        assert!(out[0].ends_with('…'));
        assert!(out[1].ends_with("oops"));
    }

    #[test]
    fn a_known_notice_is_one_info_line_per_run_not_four_warnings() {
        // Regression: PresentMon without elevation logged these four lines as
        // warnings on every game start.
        static NOTICE: Notice = Notice {
            fragments: &["requires elevated privilege", "short-running or started on another account"],
            summary: "running without elevation",
            logged: AtomicBool::new(false),
        };
        let stderr = "warning: PresentMon requires elevated privilege in order to query processes that are
                      short-running or started on another account.  Without it, those processes will
                      real problem
";
        let (mut notices, mut warnings) = (0, Vec::new());
        pump(stderr.as_bytes(), |line| {
            if NOTICE.matches(line) {
                notices += 1;
            } else {
                warnings.push(line.to_string());
            }
        });
        assert_eq!(notices, 2);
        assert_eq!(warnings, vec!["real problem"]);
    }

    #[test]
    fn a_last_line_without_newline_is_kept() {
        assert_eq!(run(b"no newline").0, vec!["no newline"]);
    }
}
