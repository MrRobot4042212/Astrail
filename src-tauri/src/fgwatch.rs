// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Foreground-change notifications, so the playtime watcher can notice a game that
//! was started outside Astrail (from Steam, a desktop shortcut…) without polling.
//!
//! `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT)`: the system
//! calls us back **in our own process** when the foreground window changes. Nothing
//! is loaded into another process (that is what `WINEVENT_INCONTEXT` does, and it is
//! never used here), so this is as anti-cheat neutral as reading the foreground
//! window on a timer — minus the timer.
//!
//! Thread ownership: the hook is installed, pumped and removed on one dedicated
//! thread, because out-of-context callbacks are delivered through the message queue
//! of the thread that called `SetWinEventHook`. That thread blocks in `GetMessageW`
//! and costs nothing while the foreground does not change. The callback does no
//! work of its own: it hands the pid to `playtime::foreground_changed`, which wakes
//! the watcher, and all matching happens there.

/// Start or stop the hook thread. Idempotent; safe to call from any thread.
pub fn set_enabled(on: bool) {
    #[cfg(windows)]
    imp::set_enabled(on);
    #[cfg(not(windows))]
    let _ = on;
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW,
        EVENT_SYSTEM_FOREGROUND, MSG, PM_NOREMOVE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
        WM_QUIT,
    };

    /// The running hook thread, if any.
    struct Running {
        stop: Arc<AtomicBool>,
        /// 0 until the thread has a message queue that `WM_QUIT` can be posted to.
        thread_id: Arc<AtomicU32>,
    }

    static STATE: Mutex<Option<Running>> = Mutex::new(None);

    /// Pid of the last foreground change passed on, so moving between two windows
    /// of the same process does not wake the watcher again.
    static LAST_PID: AtomicU32 = AtomicU32::new(0);

    /// Hook threads alive right now. More than one only for the instant an old thread
    /// takes to see its `WM_QUIT` after a quick off/on.
    static LIVE: AtomicU32 = AtomicU32::new(0);

    /// Owns the hook: removed on every exit path of the thread.
    struct Hook(HWINEVENTHOOK);

    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: `self.0` came from a successful `SetWinEventHook` on this same
            // thread, which is the only thread allowed to remove it.
            unsafe {
                let _ = UnhookWinEvent(self.0);
            }
        }
    }

    pub fn set_enabled(on: bool) {
        let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        if !on {
            if let Some(running) = state.take() {
                running.stop.store(true, Ordering::SeqCst);
                let id = running.thread_id.load(Ordering::SeqCst);
                // id == 0: the thread has not published its id yet, and will see
                // `stop` right after it does (both sides store, then load).
                if id != 0 {
                    // SAFETY: plain message post; a thread that already exited just
                    // makes the call fail.
                    let _ = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) };
                }
            }
            return;
        }
        if state.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let thread_id = Arc::new(AtomicU32::new(0));
        let (thread_stop, thread_tid) = (stop.clone(), thread_id.clone());
        // Counted from here, not from inside the thread: "spawned and not yet gone".
        LIVE.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new()
            .name("astrail-foreground".into())
            .spawn(move || {
                run(&thread_stop, &thread_tid);
                LIVE.fetch_sub(1, Ordering::SeqCst);
                // Forget this thread, unless a later `set_enabled` already replaced it.
                let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
                if state.as_ref().is_some_and(|r| Arc::ptr_eq(&r.stop, &thread_stop)) {
                    *state = None;
                }
            });
        match spawned {
            Ok(_) => *state = Some(Running { stop, thread_id }),
            Err(e) => {
                LIVE.fetch_sub(1, Ordering::SeqCst);
                log::warn!("could not start the foreground watcher: {e}");
            }
        }
    }

    fn run(stop: &AtomicBool, thread_id: &AtomicU32) {
        let mut msg = MSG::default();
        // SAFETY: `msg` outlives every call that writes to it. The first
        // `PeekMessageW` only forces the thread's message queue into existence, so a
        // `WM_QUIT` posted from now on is not lost.
        unsafe {
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if hook.is_invalid() {
                log::warn!("SetWinEventHook failed: games started outside Astrail will not be detected");
                return;
            }
            let _hook = Hook(hook);
            log::info!("foreground watcher started");

            // A game may already be in front (Astrail started after it).
            LAST_PID.store(0, Ordering::Relaxed);
            notify(crate::overlay::foreground_pid());

            // 0 = WM_QUIT, -1 = error: both end the thread.
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                DispatchMessageW(&msg);
            }
        }
        log::info!("foreground watcher stopped");
    }

    fn notify(pid: u32) {
        if pid == 0 || pid == std::process::id() {
            return;
        }
        if LAST_PID.swap(pid, Ordering::Relaxed) != pid {
            crate::playtime::foreground_changed();
        }
    }

    /// Runs on the hook thread, from inside `GetMessageW`.
    unsafe extern "system" fn on_foreground(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _event_thread: u32,
        _event_time: u32,
    ) {
        if event != EVENT_SYSTEM_FOREGROUND || hwnd.0.is_null() {
            return;
        }
        let mut pid = 0u32;
        // SAFETY: `hwnd` is the window the system just reported; a window that is
        // already gone makes the call return 0 and leaves `pid` at 0.
        unsafe {
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        notify(pid);
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::{Duration, Instant};

        fn settles_at(n: u32) -> bool {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if LIVE.load(Ordering::SeqCst) == n {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        }

        /// Needs an interactive desktop: `cargo test -- --ignored`.
        #[test]
        #[ignore]
        fn the_foreground_hook_installs_on_a_real_desktop() {
            // SAFETY: installed and removed on this thread; the callback is never
            // reached because no message is pumped in between.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(on_foreground),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            assert!(!hook.is_invalid());
            drop(Hook(hook));
        }

        #[test]
        fn the_hook_thread_always_goes_away_when_turned_off() {
            // A stop that lands before the thread has a message queue cannot be
            // delivered as WM_QUIT: the thread has to see the flag instead.
            for _ in 0..20 {
                set_enabled(true);
                set_enabled(false);
                assert!(settles_at(0), "a hook thread outlived its stop");
            }
            // And one that is already blocked in GetMessageW needs the WM_QUIT.
            set_enabled(true);
            std::thread::sleep(Duration::from_millis(150));
            set_enabled(true); // idempotent: no second thread
            assert_eq!(LIVE.load(Ordering::SeqCst), 1);
            set_enabled(false);
            assert!(settles_at(0), "WM_QUIT did not end the hook thread");
        }
    }
}
