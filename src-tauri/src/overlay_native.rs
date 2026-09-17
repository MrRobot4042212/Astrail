// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Backend-independent foreground-window helpers (Windows only).
//!
//! Previously this module also held a GDI layered-window HUD as a fallback backend.
//! That was removed on purpose: a GDI `UpdateLayeredWindow` overlay is never
//! MPO-eligible, so it always drops the game from independent-flip to composed-flip
//! and adds input lag — the exact cost the overlay is meant to avoid. The HUD is now
//! DirectComposition-only (`overlay_dcomp`); if DComp can't init the HUD is disabled
//! rather than falling back to GDI. Only the foreground helpers below survive, used by
//! the facade to gate drawing on the game actually being focused.


use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// The current foreground window handle (0 = none). Used to gate `reassert_topmost`.
pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// PID owning the current foreground window (0 = none). Lets the sampler draw the HUD
/// only while the game is actually focused — alt-tabbed out, drawing the HUD would
/// composite over the desktop (forcing composition) for nothing.
pub fn foreground_pid() -> u32 {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return 0;
        }
        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    }
}
