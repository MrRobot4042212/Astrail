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

/// Whether our own executable sits in a location the current user can write to
/// (the default Tauri/NSIS per-user install goes to `%LOCALAPPDATA%`).
///
/// This gates the elevated logon task: a `/RL HIGHEST` scheduled task pointing at
/// a user-writable path is a local privilege-escalation primitive — anything able
/// to replace that file gets silent SYSTEM-adjacent execution at every logon,
/// with no UAC prompt. When it is user-writable we refuse to create the task and
/// fall back to the ordinary (non-elevated) `Run` key.
pub fn exe_in_user_writable_location() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        // Unknown location: assume the unsafe case.
        return true;
    };
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let user_roots = ["USERPROFILE", "LOCALAPPDATA", "APPDATA", "TEMP", "PUBLIC"];
    user_roots.iter().any(|var| {
        std::env::var(var)
            .ok()
            .and_then(|dir| std::fs::canonicalize(dir).ok())
            .is_some_and(|root| exe.starts_with(&root))
    })
}

/// Name of the Task Scheduler entry used to autostart Meteor elevated.
const AUTOSTART_TASK: &str = "MeteorAutostart";

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

/// Whether the elevated logon autostart task exists.
pub fn logon_task_exists() -> bool {
    schtasks(&["/Query", "/TN", AUTOSTART_TASK]).unwrap_or(false)
}

/// Create (or overwrite) a Task Scheduler entry that launches Meteor at logon
/// with highest privileges. This is the only way Windows will autostart an app
/// that requires elevation (UAC) without a prompt at every login — a plain
/// `HKCU\...\Run` entry is silently blocked for elevated apps. Requires the
/// current process to be elevated (creating a `/RL HIGHEST` task needs admin)
/// **and** the executable to live outside a user-writable directory — see
/// `exe_in_user_writable_location`.
pub fn create_logon_task() -> Result<(), String> {
    if exe_in_user_writable_location() {
        return Err(
            "No se crea la tarea de inicio elevada: el ejecutable está en una carpeta escribible por el usuario."
                .into(),
        );
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // schtasks parses its own /TR value, so wrap the path in quotes for spaces.
    let tr = format!("\"{}\"", exe.display());
    let ok = schtasks(&[
        "/Create", "/TN", AUTOSTART_TASK, "/TR", &tr, "/SC", "ONLOGON", "/RL",
        "HIGHEST", "/F",
    ])
    .map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err("No se pudo crear la tarea de inicio automático (schtasks).".into())
    }
}

/// Remove the elevated logon autostart task. No-op (Ok) if it doesn't exist.
pub fn delete_logon_task() -> Result<(), String> {
    if !logon_task_exists() {
        return Ok(());
    }
    let ok = schtasks(&["/Delete", "/TN", AUTOSTART_TASK, "/F"]).map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err("No se pudo borrar la tarea de inicio automático (schtasks).".into())
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
