// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Process-wide Job Object for the metrics sidecars (`PresentMon.exe`,
//! `cputemp.exe`).
//!
//! Both sidecars run **elevated** and hold system resources (an ETW realtime
//! session; a kernel driver). Previously they were tracked by PID and killed
//! with `taskkill /F /PID` on exit — which loses the race against PID reuse
//! (the PID may belong to something else by then) and, worse, does nothing at
//! all if Astrail is killed, crashes, or aborts on panic (`panic = "abort"`),
//! leaving an orphaned elevated process behind.
//!
//! A Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the kernel do
//! it: when our last handle to the job goes away — including when the process
//! dies for any reason — every process assigned to it is terminated. The job
//! contains only our own sidecars; a game is never assigned to it.


use std::process::Child;
use std::sync::OnceLock;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

/// The job handle as a raw `usize` (HANDLE is a raw pointer and not `Sync`).
/// `0` means "job unavailable" — creation failed, and the caller degrades to the
/// explicit `Child::kill` it already does.
static JOB: OnceLock<usize> = OnceLock::new();

/// Create the job on first use and keep it for the process lifetime. The handle
/// is intentionally never closed: closing it is exactly what kills the sidecars,
/// so that must happen only when the process itself goes away.
fn job() -> Option<HANDLE> {
    let raw = *JOB.get_or_init(|| unsafe {
        let Ok(handle) = CreateJobObjectW(None, None) else {
            eprintln!("[jobobj] CreateJobObjectW failed; sidecars fall back to explicit kill");
            return 0;
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` lives across the call and its size matches the class.
        let set = SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set.is_err() {
            eprintln!("[jobobj] SetInformationJobObject failed; job not used");
            let _ = CloseHandle(handle);
            return 0;
        }
        handle.0 as usize
    });
    (raw != 0).then_some(HANDLE(raw as *mut core::ffi::c_void))
}

/// Assign a freshly spawned child to the kill-on-close job. Best effort: on
/// failure the child simply isn't job-managed and the controller's own
/// `Child::kill` remains the cleanup path.
pub fn assign(child: &Child) -> bool {
    let Some(job) = job() else { return false };
    // SAFETY: the PID belongs to a child we just spawned and still hold, so it
    // cannot have been reused; the handle is closed on every path below.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, child.id())
        else {
            eprintln!("[jobobj] OpenProcess failed for pid {}", child.id());
            return false;
        };
        let ok = AssignProcessToJobObject(job, process).is_ok();
        if !ok {
            eprintln!("[jobobj] AssignProcessToJobObject failed for pid {}", child.id());
        }
        let _ = CloseHandle(process);
        ok
    }
}
