// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Cheap global CPU% and RAM via direct Win32, for the per-second metrics sampler.
//!
//! The overlay only shows **global CPU usage** and **used/total physical RAM**, so the
//! full `sysinfo::System` refresh (process tables, components, disks…) was overkill to
//! run every tick while a game is in the foreground. `GetSystemTimes` (idle/kernel/user
//! deltas) and `GlobalMemoryStatusEx` give exactly those two numbers in a couple of
//! syscalls, with no allocation and no process enumeration.
//!
//! Windows-only. These helpers replaced the `sysinfo` crate entirely, which
//! also removed its unguarded subtraction that panicked debug builds.


use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

const MB: u64 = 1024 * 1024;

fn ft_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Stateful global-CPU% meter. Each `pct()` returns the busy percentage over the
/// interval since the previous call, computed from `GetSystemTimes` deltas.
///
/// The first call after `new()`/`reset()` has nothing to diff against and returns
/// `None`, so the HUD hides the row instead of showing a fabricated `CPU 0%`.
#[derive(Default)]
pub struct CpuMeter {
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    primed: bool,
}

impl CpuMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the previous sample. Called while the sampler is parked, so the first
    /// reading after a game comes back is not averaged over the whole idle period.
    pub fn reset(&mut self) {
        self.primed = false;
    }

    /// Global CPU usage in 0..=100 since the last call, or `None` when there is no
    /// valid interval to report (priming call, failed query, zero elapsed time).
    pub fn pct(&mut self) -> Option<f32> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: three valid out-parameters for a plain system query.
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
            return None;
        }
        self.update(ft_to_u64(idle), ft_to_u64(kernel), ft_to_u64(user))
    }

    /// Pure delta step behind `pct`, split out so it can be tested without the OS.
    fn update(&mut self, idle: u64, kernel: u64, user: u64) -> Option<f32> {
        let primed = std::mem::replace(&mut self.primed, true);
        let (d_idle, d_kernel, d_user) = (
            idle.saturating_sub(self.prev_idle),
            kernel.saturating_sub(self.prev_kernel),
            user.saturating_sub(self.prev_user),
        );
        self.prev_idle = idle;
        self.prev_kernel = kernel;
        self.prev_user = user;
        if !primed {
            return None;
        }

        // `kernel` time already INCLUDES idle time, so busy = (kernel+user) - idle.
        let total = d_kernel + d_user;
        if total == 0 {
            return None;
        }
        let busy = total.saturating_sub(d_idle);
        Some(((busy as f64 / total as f64) * 100.0) as f32)
    }
}

/// `(used_mb, total_mb)` physical RAM, or `(0, 0)` if the query fails.
pub fn mem_mb() -> (u64, u64) {
    let mut ms = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut ms) }.is_ok() {
        let total = ms.ullTotalPhys / MB;
        let used = ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys) / MB;
        (used, total)
    } else {
        (0, 0)
    }
}

// --- System description (replaces sysinfo) ---------------------------------

/// CPU brand string, from the same registry value the OS reports it in.
pub fn cpu_brand() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .ok()?;
    let brand: String = key.get_value("ProcessorNameString").ok()?;
    let brand = brand.trim().to_string();
    (!brand.is_empty()).then_some(brand)
}

/// (physical cores, logical processors).
pub fn cpu_counts() -> (usize, usize) {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };
    use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    let logical = {
        let mut info = SYSTEM_INFO::default();
        // SAFETY: plain out-parameter query.
        unsafe { GetSystemInfo(&mut info) };
        info.dwNumberOfProcessors as usize
    };

    // Physical cores: walk the variable-length RelationProcessorCore records.
    let mut physical = 0usize;
    // SAFETY: the first call only asks for the required buffer size; the second
    // fills a buffer of exactly that size, and we walk it by each record's own
    // `Size` field, which is how the API defines the layout.
    unsafe {
        let mut len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len > 0 {
            let mut buf = vec![0u8; len as usize];
            if GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
                &mut len,
            )
            .is_ok()
            {
                let mut offset = 0usize;
                while offset + std::mem::size_of::<u32>() * 2 <= buf.len() {
                    let record = buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
                    let size = (*record).Size as usize;
                    if size == 0 || offset + size > buf.len() {
                        break;
                    }
                    physical += 1;
                    offset += size;
                }
            }
        }
    }
    if physical == 0 {
        physical = logical;
    }
    (physical, logical)
}

/// Windows product name and display version, e.g. "Windows 11 Pro 24H2".
pub fn os_description() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    let product: String = key.get_value("ProductName").unwrap_or_default();
    let display: String = key
        .get_value("DisplayVersion")
        .or_else(|_| key.get_value("ReleaseId"))
        .unwrap_or_default();
    let joined = format!("{} {}", product.trim(), display.trim())
        .trim()
        .to_string();
    (!joined.is_empty()).then_some(joined)
}

/// Fixed drives with their size and free space, in bytes.
pub fn drives() -> Vec<(String, String, u64, u64)> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;

    let mut out = Vec::new();
    // SAFETY: each call is a plain query over a NUL-terminated drive root we
    // build here; the string buffers are sized as the API requires.
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = format!("{letter}:\\");
            let root_w = HSTRING::from(root.as_str());
            if GetDriveTypeW(&root_w) != DRIVE_FIXED {
                continue; // skip optical drives, removable media, network shares
            }
            let (mut free, mut total) = (0u64, 0u64);
            if GetDiskFreeSpaceExW(&root_w, None, Some(&mut total), Some(&mut free)).is_err() {
                continue;
            }
            let mut fs_name = [0u16; 32];
            let mut label = [0u16; 128];
            let fs = if GetVolumeInformationW(
                &root_w,
                Some(&mut label),
                None,
                None,
                None,
                Some(&mut fs_name),
            )
            .is_ok()
            {
                String::from_utf16_lossy(&fs_name)
                    .trim_end_matches('\0')
                    .to_string()
            } else {
                String::new()
            };
            out.push((format!("{letter}:"), fs, total, free));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_priming_call_reports_nothing_instead_of_zero() {
        // Regression (MT6): the first tick drew `CPU 0%` on the HUD.
        let mut m = CpuMeter::new();
        assert_eq!(m.update(100, 200, 100), None);
        // +100 idle out of +400 total (kernel 300 incl. idle, user 100) → 75 % busy.
        assert_eq!(m.update(200, 500, 200), Some(75.0));
    }

    #[test]
    fn reset_re_primes_instead_of_averaging_over_the_idle_gap() {
        let mut m = CpuMeter::new();
        m.update(0, 0, 0);
        assert!(m.update(10, 20, 20).is_some());
        m.reset();
        assert_eq!(m.update(1_000_000, 2_000_000, 1_000_000), None);
        assert_eq!(m.update(1_000_050, 2_000_100, 1_000_100), Some(75.0));
    }

    #[test]
    fn no_elapsed_time_is_not_reported_as_idle() {
        let mut m = CpuMeter::new();
        m.update(5, 5, 5);
        assert_eq!(m.update(5, 5, 5), None);
    }
}
