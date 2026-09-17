// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Process elevation helpers for the admin-only metrics (CPU temp via the LHM
//! sidecar, NVIDIA FPS via PresentMon). Windows can't elevate a running process,
//! so the UI offers a "Restart as admin" action that relaunches via the `runas`
//! verb (UAC prompt); the old instance then exits.


use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// True if this process is running with an elevated (administrator) token.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Relaunch our own executable elevated via the `runas` verb. Returns Ok once the
/// elevated process has been requested (the caller should then exit this one). An
/// `Err` means the user declined UAC or the launch failed.
pub fn relaunch_elevated() -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w = HSTRING::from(exe.as_os_str());
    // The single-instance guard would make the elevated copy hand off to this one
    // and exit while this one is about to exit too; it waits for us instead.
    let params = HSTRING::from(format!("{AWAIT_EXIT_ARG}{}", std::process::id()));

    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR(params.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns an HINSTANCE; values <= 32 indicate failure.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("No se pudo reiniciar como administrador (UAC cancelado).".into())
    }
}

/// Argument an elevated relaunch carries: `--await-exit=<pid of the old instance>`.
const AWAIT_EXIT_ARG: &str = "--await-exit=";

/// How long a relaunched instance waits for the old one before starting anyway.
const AWAIT_EXIT_TIMEOUT_MS: u32 = 10_000;

/// The pid an elevated relaunch was asked to wait for, if any.
fn await_exit_pid<I: IntoIterator<Item = String>>(args: I) -> Option<u32> {
    args.into_iter()
        .find_map(|a| a.strip_prefix(AWAIT_EXIT_ARG).and_then(|p| p.parse().ok()))
        .filter(|pid| *pid != 0)
}

/// Called first thing in `run()`: if this process is the elevated copy started by
/// `relaunch_elevated`, block until the old instance has exited, so the
/// single-instance guard does not see it and forward to a process about to quit.
/// Bounded, and best-effort: if the pid cannot be opened it is already gone.
pub fn await_previous_instance() {
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

    let Some(pid) = await_exit_pid(std::env::args()) else { return };
    // SAFETY: OpenProcess has no preconditions; the returned handle is owned here,
    // waited on, and closed exactly once.
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(handle, AWAIT_EXIT_TIMEOUT_MS);
            let _ = CloseHandle(handle);
        }
    }
}

/// Name of the Task Scheduler entry older builds created to autostart Meteor
/// elevated. It is no longer created: autostart is the ordinary `Run` key only,
/// because a `/RL HIGHEST` logon task restarts an elevated launcher at every
/// logon without UAC, and every game spawned from it inherits the admin token.
const LEGACY_AUTOSTART_TASK: &str = "MeteorAutostart";

/// Run `schtasks` without flashing a console window, returning whether it
/// exited successfully.
fn schtasks(args: &[&str]) -> std::io::Result<bool> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW so the console of schtasks.exe never flashes.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new(crate::files::system_exe("schtasks.exe"))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(status.success())
}

/// Remove the elevated logon task left by an older build. Returns `true` when a
/// task existed and is now gone, so the caller can move autostart to the `Run`
/// key. Only an elevated process can delete a `/RL HIGHEST` task, and such a task
/// only ever launches Meteor elevated, so callers skip this when not elevated:
/// the next logon launch performs it, and normal launches spawn nothing.
pub fn remove_legacy_logon_task() -> bool {
    if !schtasks(&["/Query", "/TN", LEGACY_AUTOSTART_TASK]).unwrap_or(false) {
        return false;
    }
    match schtasks(&["/Delete", "/TN", LEGACY_AUTOSTART_TASK, "/F"]) {
        Ok(true) => true,
        Ok(false) => {
            eprintln!("[elevation] schtasks could not delete the legacy {LEGACY_AUTOSTART_TASK} task");
            false
        }
        Err(e) => {
            eprintln!("[elevation] schtasks failed to start: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_elevated_relaunch_waits_for_the_instance_that_started_it() {
        // Regression (W2): with the single-instance guard, the elevated copy found
        // the old instance still alive, handed off to it and exited, and the old one
        // then quit as planned, leaving no Meteor running.
        assert_eq!(await_exit_pid(args(&["meteor.exe", "--await-exit=4242"])), Some(4242));
        assert_eq!(await_exit_pid(args(&["meteor.exe"])), None);
        assert_eq!(await_exit_pid(args(&["meteor.exe", "--await-exit=0"])), None);
        assert_eq!(await_exit_pid(args(&["meteor.exe", "--await-exit=abc"])), None);
    }
}
