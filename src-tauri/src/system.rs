//! System / hardware info for the in-app "Mi equipo" panel and the metrics GPU
//! picker. CPU/RAM/OS/disks come straight from Win32 and the registry
//! (`sysstat.rs`); the motherboard from the BIOS registry key; displays from
//! Win32 GDI; and the GPU list (with metric-capable keys for the picker) from
//! NVML (NVIDIA) and DXGI (everything else).

use serde::Serialize;

/// A GPU as shown in the panel. `key` ("nvml:<i>" / "pci:<AMD PnP id fragment>") is
/// set only for metric-capable GPUs — those can be picked for the overlay; empty
/// otherwise.
#[derive(Serialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub vram_mb: u64,
    /// "integrated" | "discrete" | "" (unknown); the panel translates it.
    pub kind: String,
    pub key: String,
}

#[derive(Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub fs: String,
    pub total_mb: u64,
    pub available_mb: u64,
}

#[derive(Serialize)]
pub struct DisplayInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
}

/// Why the in-game overlay may be costing performance: the live composition health
/// plus the system-config levers that decide whether Windows grants the HUD a hardware
/// overlay plane (MPO). Surfaced in the overlay settings so the user can fix their config.
#[derive(Serialize)]
pub struct MpoDiagnostics {
    /// Live overlay health: 0 unknown, 1 free (HUD on a hardware plane), 2 costing
    /// (DWM compositing the HUD → the game loses independent-flip).
    pub health: u8,
    /// Number of active monitors. Multi-monitor is the most common MPO blocker.
    pub monitors: u32,
    /// Refresh rate (Hz) of each active monitor.
    pub refresh_rates: Vec<u32>,
    /// True if the active monitors run at different refresh rates — a frequent MPO
    /// blocker (DWM can't independent-flip cleanly across mixed refresh).
    pub mixed_refresh: bool,
    /// Hardware-accelerated GPU scheduling (HAGS) enabled — often required for MPO.
    /// `None` if it couldn't be determined.
    pub hags: Option<bool>,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub cpu: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub ram_total_mb: u64,
    pub os: String,
    pub motherboard: Option<String>,
    pub gpus: Vec<GpuInfo>,
    pub disks: Vec<DiskInfo>,
    pub displays: Vec<DisplayInfo>,
}

const MB: u64 = 1024 * 1024;

/// Gather everything for the panel. Best-effort: any source that fails is just
/// omitted (empty list / `None`), never an error.
pub fn collect() -> SystemInfo {
    let cpu = crate::sysstat::cpu_brand().unwrap_or_else(|| "Desconocido".to_string());
    let os = crate::sysstat::os_description().unwrap_or_else(|| "Desconocido".to_string());
    let (cores, threads) = crate::sysstat::cpu_counts();
    let (_, ram_total_mb) = crate::sysstat::mem_mb();

    let disks = crate::sysstat::drives()
        .into_iter()
        .map(|(name, fs, total, available)| DiskInfo {
            name,
            fs,
            total_mb: total / MB,
            available_mb: available / MB,
        })
        .collect();

    SystemInfo {
        cpu,
        cpu_cores: cores,
        cpu_threads: threads,
        ram_total_mb,
        os,
        motherboard: motherboard(),
        gpus: gpus(),
        disks,
        displays: displays(),
    }
}

/// Every hardware GPU, the metric-capable ones tagged with their picker key: NVIDIA
/// through NVML, AMD through the sidecar (keyed by PnP id), the rest listed only.
fn gpus() -> Vec<GpuInfo> {
    let mut out = Vec::new();

    // NVIDIA via NVML.
    if let Ok(nvml) = nvml_wrapper::Nvml::init() {
        if let Ok(count) = nvml.device_count() {
            for i in 0..count {
                if let Ok(dev) = nvml.device_by_index(i) {
                    let name = dev.name().unwrap_or_else(|_| "NVIDIA GPU".to_string());
                    let vram_mb = dev.memory_info().map(|m| m.total / MB).unwrap_or(0);
                    out.push(GpuInfo {
                        name,
                        vendor: "NVIDIA".to_string(),
                        vram_mb,
                        kind: "discrete".to_string(),
                        key: format!("nvml:{i}"),
                    });
                }
            }
        }
    }

    // Everything else via DXGI. NVIDIA cards only when NVML did not list them.
    #[cfg(windows)]
    {
        let nvml_listed = !out.is_empty();
        for adapter in dxgi::hardware_adapters() {
            if nvml_listed && adapter.vendor_id == VENDOR_NVIDIA {
                continue;
            }
            // Two identical AMD cards share a key and the sidecar reads the first one
            // it matches, so only that one is offered for metrics.
            let key = pci_key(
                adapter.vendor_id,
                adapter.device_id,
                adapter.subsys_id,
                adapter.revision,
            )
            .filter(|key| !out.iter().any(|gpu| gpu.key == *key))
            .unwrap_or_default();
            out.push(GpuInfo {
                name: adapter.name.clone(),
                vendor: vendor_name(adapter.vendor_id).to_string(),
                vram_mb: adapter.dedicated_vram_bytes / MB,
                kind: match dxgi::is_integrated(&adapter) {
                    Some(true) => "integrated",
                    Some(false) => "discrete",
                    None => "",
                }
                .to_string(),
                key,
            });
        }
    }

    out
}

const VENDOR_AMD: u32 = 0x1002;
const VENDOR_NVIDIA: u32 = 0x10DE;
const VENDOR_INTEL: u32 = 0x8086;

fn vendor_name(vendor_id: u32) -> &'static str {
    match vendor_id {
        VENDOR_AMD => "AMD",
        VENDOR_NVIDIA => "NVIDIA",
        VENDOR_INTEL => "Intel",
        _ => "",
    }
}

/// The ids of a PCI adapter, as DXGI reports them.
#[cfg_attr(not(windows), allow(dead_code))]
struct PciAdapter {
    name: String,
    vendor_id: u32,
    device_id: u32,
    subsys_id: u32,
    revision: u32,
    dedicated_vram_bytes: u64,
    #[cfg(windows)]
    handle: windows::Win32::Graphics::Dxgi::IDXGIAdapter1,
}

/// Picker key for an AMD adapter: the start of its PnP instance id, which the sidecar
/// matches against LibreHardwareMonitor's device id. None for other vendors (no
/// metrics without NVML).
#[cfg_attr(not(windows), allow(dead_code))]
fn pci_key(vendor_id: u32, device_id: u32, subsys_id: u32, revision: u32) -> Option<String> {
    (vendor_id == VENDOR_AMD).then(|| {
        format!(
            "pci:VEN_{vendor_id:04X}&DEV_{device_id:04X}&SUBSYS_{subsys_id:08X}&REV_{revision:02X}"
        )
    })
}

/// Picker keys of the AMD GPUs the sidecar can read (see `pci_key`). Cheap enough for
/// the sampler to call once per game: no device is created.
#[cfg(windows)]
pub fn amd_gpu_keys() -> Vec<String> {
    dxgi::hardware_adapters()
        .iter()
        .filter_map(|a| pci_key(a.vendor_id, a.device_id, a.subsys_id, a.revision))
        .collect()
}

#[cfg(windows)]
mod dxgi {
    use super::PciAdapter;
    use std::sync::{Mutex, PoisonError};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_FLAG,
        D3D11_FEATURE_D3D11_OPTIONS2, D3D11_FEATURE_DATA_D3D11_OPTIONS2, D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };

    /// Hardware adapters in DXGI order, skipping software ones (Microsoft Basic Render
    /// Driver). Empty when DXGI is unavailable.
    pub(super) fn hardware_adapters() -> Vec<PciAdapter> {
        // SAFETY: plain factory creation; the returned interface owns its reference.
        let factory = match unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
            Ok(factory) => factory,
            Err(e) => {
                eprintln!("[system] DXGI unavailable, no GPU listed and no AMD metrics: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        // EnumAdapters1 fails with DXGI_ERROR_NOT_FOUND past the last adapter.
        for index in 0.. {
            // SAFETY: `factory` is a live interface; any index is valid to ask for.
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                break;
            };
            // SAFETY: `adapter` is a live interface returned just above.
            let desc = match unsafe { adapter.GetDesc1() } {
                Ok(desc) => desc,
                Err(e) => {
                    eprintln!("[system] skipping GPU adapter {index}: {e}");
                    continue;
                }
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let name = &desc.Description;
            let name_len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            out.push(PciAdapter {
                name: String::from_utf16_lossy(&name[..name_len]).trim().to_string(),
                vendor_id: desc.VendorId,
                device_id: desc.DeviceId,
                subsys_id: desc.SubSysId,
                revision: desc.Revision,
                dedicated_vram_bytes: desc.DedicatedVideoMemory as u64,
                handle: adapter,
            });
        }
        out
    }

    /// Whether the adapter shares system memory (integrated GPU). None when no D3D11
    /// device can be created on it. Creating the device loads the driver's user-mode
    /// DLL and can wake a GPU that was powered down, so each adapter is asked once per
    /// run, and only for the panel, never by the sampler.
    pub(super) fn is_integrated(adapter: &PciAdapter) -> Option<bool> {
        type Ids = (u32, u32, u32, u32);
        static ASKED: Mutex<Vec<(Ids, Option<bool>)>> = Mutex::new(Vec::new());
        let ids = (adapter.vendor_id, adapter.device_id, adapter.subsys_id, adapter.revision);
        let mut asked = ASKED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, answer)) = asked.iter().find(|(known, _)| *known == ids) {
            return *answer;
        }
        let answer = query_integrated(adapter);
        asked.push((ids, answer));
        answer
    }

    fn query_integrated(adapter: &PciAdapter) -> Option<bool> {
        let mut device: Option<ID3D11Device> = None;
        // SAFETY: `adapter.handle` is a live adapter; with an explicit adapter the
        // driver type must be UNKNOWN. The device is released when `device` drops.
        unsafe {
            if let Err(e) = D3D11CreateDevice(
                &adapter.handle,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            ) {
                eprintln!("[system] no D3D11 device on {}, GPU type unknown: {e}", adapter.name);
                return None;
            }
        }
        let device = device?;
        let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS2::default();
        // SAFETY: `options` is the struct this feature expects and the size passed
        // is its own.
        unsafe {
            device
                .CheckFeatureSupport(
                    D3D11_FEATURE_D3D11_OPTIONS2,
                    &mut options as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS2>() as u32,
                )
                .ok()?;
        }
        Some(options.UnifiedMemoryArchitecture.as_bool())
    }
}

/// Motherboard manufacturer + product from the BIOS registry key (no WMI).
#[cfg(windows)]
fn motherboard() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let bios = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"HARDWARE\DESCRIPTION\System\BIOS")
        .ok()?;
    let vendor: String = bios.get_value("BaseBoardManufacturer").unwrap_or_default();
    let product: String = bios.get_value("BaseBoardProduct").unwrap_or_default();
    let joined = format!("{} {}", vendor.trim(), product.trim());
    let joined = joined.trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

#[cfg(not(windows))]
fn motherboard() -> Option<String> {
    None
}

/// Gather the overlay MPO diagnostics: live health + the config levers (monitor count,
/// mixed refresh, HAGS) that decide whether Windows can put the HUD on a hardware plane.
pub fn mpo_diagnostics() -> MpoDiagnostics {
    let disp = displays();
    let refresh_rates: Vec<u32> = disp.iter().map(|d| d.refresh_hz).collect();
    let mut uniq = refresh_rates.clone();
    uniq.sort_unstable();
    uniq.dedup();
    MpoDiagnostics {
        health: crate::metrics::overlay_health(),
        monitors: disp.len() as u32,
        mixed_refresh: uniq.len() > 1,
        refresh_rates,
        hags: hags_enabled(),
    }
}

/// Whether Hardware-accelerated GPU scheduling is enabled (`HwSchMode == 2`). `None`
/// if the value is absent / unreadable (driver default — state unknown).
#[cfg(windows)]
fn hags_enabled() -> Option<bool> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers")
        .ok()?;
    let mode: u32 = key.get_value("HwSchMode").ok()?;
    Some(mode == 2)
}

#[cfg(not(windows))]
fn hags_enabled() -> Option<bool> {
    None
}

/// Active displays (resolution + refresh) via Win32 GDI.
#[cfg(windows)]
fn displays() -> Vec<DisplayInfo> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW,
        DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, ENUM_CURRENT_SETTINGS,
    };

    fn wide_to_string(w: &[u16]) -> String {
        let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
        String::from_utf16_lossy(&w[..end])
    }

    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        let mut dev = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        let ok = unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dev, 0) };
        if !ok.as_bool() {
            break;
        }
        i += 1;

        let attached = (dev.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP).0 != 0;
        if !attached {
            continue;
        }
        let primary = (dev.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE).0 != 0;

        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let got = unsafe {
            EnumDisplaySettingsW(
                PCWSTR::from_raw(dev.DeviceName.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
        };
        if !got.as_bool() {
            continue;
        }
        let mut name = wide_to_string(&dev.DeviceString);
        let mut mon = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        let ok_mon = unsafe { EnumDisplayDevicesW(PCWSTR::from_raw(dev.DeviceName.as_ptr()), 0, &mut mon, 0) };
        if ok_mon.as_bool() {
            let mon_name = wide_to_string(&mon.DeviceString);
            if !mon_name.is_empty() {
                name = mon_name;
            }
        }

        out.push(DisplayInfo {
            name,
            width: mode.dmPelsWidth,
            height: mode.dmPelsHeight,
            refresh_hz: mode.dmDisplayFrequency,
            primary,
        });
    }
    out
}

#[cfg(not(windows))]
fn displays() -> Vec<DisplayInfo> {
    Vec::new()
}

/// The Windows user's display/full name (e.g. "Diego Chicoma") via GetUserNameExW
/// with `NameDisplay`. Returns `None` if it's empty or the call fails (common on
/// plain local accounts with no full name set), so the caller falls back to the
/// login name.
#[cfg(windows)]
pub fn display_name() -> Option<String> {
    use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};

    // First call with a zero size fails and reports the buffer length needed.
    let mut size: u32 = 0;
    unsafe { GetUserNameExW(NameDisplay, None, &mut size) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u16; size as usize];
    let ok = unsafe {
        GetUserNameExW(
            NameDisplay,
            Some(windows::core::PWSTR(buf.as_mut_ptr())),
            &mut size,
        )
    };
    if !ok {
        return None;
    }
    let name = String::from_utf16_lossy(&buf[..size as usize]);
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_amd_key_is_the_start_of_its_pnp_instance_id() {
        // PCI\VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0\6&2B36B191&0&00000009
        assert_eq!(
            pci_key(0x1002, 0x7550, 0x8811_1EAE, 0xC0).as_deref(),
            Some("pci:VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0")
        );
        // Leading zeros are kept, so the fragment still lines up with the PnP id.
        assert_eq!(
            pci_key(0x1002, 0x164E, 0x0001_0043, 0x07).as_deref(),
            Some("pci:VEN_1002&DEV_164E&SUBSYS_00010043&REV_07")
        );
    }

    #[test]
    fn only_amd_gpus_get_a_pci_key() {
        assert_eq!(pci_key(0x10DE, 0x2684, 0x1234_5678, 0xA1), None);
        assert_eq!(pci_key(0x8086, 0x46A6, 0, 0x0C), None);
    }
}
