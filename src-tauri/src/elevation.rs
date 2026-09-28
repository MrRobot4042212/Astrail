// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Process elevation helpers for the admin-only metrics. Windows can't elevate a
//! running process, so the UI offers a "Restart as admin" action that relaunches
//! via the `runas` verb (UAC prompt); the old instance then exits.
//!
//! Only CPU temperature (LHM sidecar, kernel driver) truly needs admin. FPS via
//! PresentMon needs an ETW realtime session, which Windows also grants to members
//! of the built-in **Performance Log Users** group (`can_trace_etw`).


use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// True if this process's token has the built-in Performance Log Users group
/// (`S-1-5-32-559`) enabled.
///
/// Looked up by well-known SID, never by name: the group name is localized
/// ("Usuarios del registro de rendimiento" on Spanish Windows). Membership is
/// fixed at logon, so it only has to be read once per process.
pub fn in_performance_log_users() -> bool {
    use windows::core::BOOL;
    use windows::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, WinBuiltinPerfLoggingUsersSid, PSID,
        SECURITY_MAX_SID_SIZE,
    };
    let mut buf = [0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut size = buf.len() as u32;
    let sid = PSID(buf.as_mut_ptr().cast());
    // SAFETY: `buf` is SECURITY_MAX_SID_SIZE bytes, the maximum any SID needs, and
    // `size` says so; the SID is only used while `buf` is alive.
    if unsafe { CreateWellKnownSid(WinBuiltinPerfLoggingUsersSid, None, Some(sid), &mut size) }.is_err() {
        return false;
    }
    let mut member = BOOL(0);
    // SAFETY: `None` = the calling thread's effective token; `sid` is valid (above).
    let ok = unsafe { CheckTokenMembership(None, sid, &mut member) }.is_ok();
    ok && member.as_bool()
}

/// Whether PresentMon can open its ETW realtime session from this process:
/// elevated, or a member of Performance Log Users (the documented alternative,
/// PresentMon `MainThread.cpp`). Measured 2026-09-21 on a non-elevated token in
/// the group: frames, display and `msGPUActive` columns all arrive, and the
/// session can be stopped by name without admin.
pub fn can_trace_etw() -> bool {
    is_elevated() || in_performance_log_users()
}

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

    // ShellExecute may hand the verb to a shell extension, which wants an
    // apartment; this runs on a pool thread, not the main one.
    let _com = crate::launcher::desktop_shell::ComScope::enter();
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
        Err("Could not restart as administrator (UAC prompt declined or failed).".into())
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

/// Name of the single-instance plugin's mutex (tauri-plugin-single-instance 2.x,
/// `platform_impl/windows.rs`, built without its `semver` feature).
fn instance_mutex_name(identifier: &str) -> String {
    format!("{identifier}-sim")
}

/// Whether another Astrail is running **elevated** while this launch is not.
///
/// The single-instance plugin creates its mutex with default security and takes
/// only `ERROR_ALREADY_EXISTS` as "already running". An elevated process's objects
/// are closed to a medium-integrity one, so a normal launch next to an elevated
/// Astrail got `ERROR_ACCESS_DENIED` instead and started as a full second
/// instance: two trays, two HUDs, two PresentMon sessions under one name stopping
/// each other, every session recorded twice (2026-09-27 audit, X-I2). Any other
/// outcome (no instance, or one this process can open) is the plugin's to handle.
pub fn elevated_instance_running(identifier: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

    let name = HSTRING::from(instance_mutex_name(identifier));
    // SAFETY: OpenMutexW has no preconditions; a returned handle is closed once.
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) } {
        Ok(handle) => {
            // SAFETY: `handle` was just returned by OpenMutexW and is not used again.
            let _ = unsafe { CloseHandle(handle) };
            false
        }
        Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
    }
}

/// `(title, text)` of the notice a launch shows when an elevated Astrail already
/// runs. It cannot hand over to that instance the way the plugin does: Windows
/// blocks messages from a medium-integrity process to an elevated one, and opening
/// that door would expose the plugin's message handler, which reads the payload
/// without bounds, to every unelevated process.
fn elevated_instance_notice(spanish: bool) -> (&'static str, &'static str) {
    if spanish {
        (
            "Astrail",
            "Astrail ya se está ejecutando como administrador.\n\n\
             Ábrelo desde su icono en la bandeja del sistema, o ciérralo desde ahí \
             antes de abrirlo de nuevo sin permisos de administrador.",
        )
    } else {
        (
            "Astrail",
            "Astrail is already running as administrator.\n\n\
             Open it from its icon in the system tray, or quit it from there before \
             opening it again without administrator rights.",
        )
    }
}

/// Tell the user an elevated Astrail is already running (see
/// `elevated_instance_running`). Blocks until the notice is dismissed.
pub fn show_elevated_instance_notice(spanish: bool) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
    };
    let (title, text) = elevated_instance_notice(spanish);
    // SAFETY: both strings outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
        );
    }
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

/// Name of the Task Scheduler entry older builds created to autostart the app
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
/// only ever launches the app elevated, so callers skip this when not elevated:
/// the next logon launch performs it, and normal launches spawn nothing.
pub fn remove_legacy_logon_task() -> bool {
    if !schtasks(&["/Query", "/TN", LEGACY_AUTOSTART_TASK]).unwrap_or(false) {
        return false;
    }
    match schtasks(&["/Delete", "/TN", LEGACY_AUTOSTART_TASK, "/F"]) {
        Ok(true) => true,
        Ok(false) => {
            log::warn!("schtasks could not delete the legacy {LEGACY_AUTOSTART_TASK} task");
            false
        }
        Err(e) => {
            log::warn!("schtasks failed to start: {e}");
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
    fn the_instance_check_opens_the_plugins_own_mutex() {
        // The plugin names it `<identifier>-sim` (single-instance 2.4.4, no
        // `semver` feature). A different name would find nothing and let a second
        // instance start next to an elevated one again (X-I2).
        assert_eq!(instance_mutex_name("com.alfonso.meteor"), "com.alfonso.meteor-sim");
        // Nothing holds this name: not "an elevated instance is running".
        assert!(!elevated_instance_running("astrail-test-no-such-instance"));
    }

    #[test]
    fn a_mutex_this_process_can_open_is_left_to_the_plugin() {
        use windows::core::HSTRING;
        use windows::Win32::System::Threading::CreateMutexW;
        let id = format!("astrail-test-{}", std::process::id());
        let name = HSTRING::from(instance_mutex_name(&id));
        // SAFETY: default security, not initially owned; closed below.
        let mutex = unsafe { CreateMutexW(None, false, &name) }.expect("test mutex");
        assert!(!elevated_instance_running(&id));
        // SAFETY: created above, not used again.
        let _ = unsafe { CloseHandle(mutex) };
    }

    #[test]
    fn the_elevated_instance_notice_says_where_to_find_it() {
        for spanish in [true, false] {
            let (title, text) = elevated_instance_notice(spanish);
            assert_eq!(title, "Astrail");
            assert!(text.contains(if spanish { "bandeja" } else { "tray" }), "{text}");
        }
    }

    #[test]
    fn performance_log_users_membership_agrees_with_whoami() {
        // `whoami /groups` lists the group by SID whatever the display language.
        // 559 is never a deny-only SID (UAC filters only admin-class groups), so
        // "listed" and "member" are the same thing for it.
        let whoami = std::path::Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
            .join(r"System32\whoami.exe");
        let Ok(out) = std::process::Command::new(whoami).args(["/groups", "/fo", "csv"]).output() else {
            return; // no whoami (stripped image): nothing to compare against
        };
        let listed = String::from_utf8_lossy(&out.stdout).contains("\"S-1-5-32-559\"");
        assert_eq!(in_performance_log_users(), listed);
    }

    #[test]
    fn an_elevated_relaunch_waits_for_the_instance_that_started_it() {
        // Regression (W2): with the single-instance guard, the elevated copy found
        // the old instance still alive, handed off to it and exited, and the old one
        // then quit as planned, leaving no Astrail running.
        assert_eq!(await_exit_pid(args(&["astrail.exe", "--await-exit=4242"])), Some(4242));
        assert_eq!(await_exit_pid(args(&["astrail.exe"])), None);
        assert_eq!(await_exit_pid(args(&["astrail.exe", "--await-exit=0"])), None);
        assert_eq!(await_exit_pid(args(&["astrail.exe", "--await-exit=abc"])), None);
    }
}
