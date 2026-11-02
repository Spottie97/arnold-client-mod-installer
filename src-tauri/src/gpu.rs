use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vram_gb: f64,
    pub dedicated: bool,
}

impl GpuInfo {
    pub fn unknown() -> Self {
        Self {
            name: "Unknown GPU".into(),
            vram_gb: 0.0,
            dedicated: false,
        }
    }
}

fn mb_to_gb(mb: f64) -> f64 {
    ((mb / 1024.0) * 10.0).round() / 10.0
}

fn bytes_to_gb(bytes: u64) -> f64 {
    ((bytes as f64 / 1024.0 / 1024.0 / 1024.0) * 10.0).round() / 10.0
}

/// `Win32_VideoController.AdapterRAM` is a uint32. Drivers report this when the
/// real framebuffer is larger than 4 GiB.
pub const ADAPTER_RAM_CAP_SENTINEL: u64 = 0xFFF0_0000;
pub const FOUR_GIB: u64 = 4 * 1024 * 1024 * 1024;

pub fn adapter_ram_is_capped(bytes: u64) -> bool {
    bytes == ADAPTER_RAM_CAP_SENTINEL || bytes == FOUR_GIB || bytes == u32::MAX as u64
}

/// Registry QWORD, then DXGI dedicated memory, then AdapterRAM.
/// A capped AdapterRAM reading never replaces a larger value from the other sources.
pub fn resolve_vram_bytes(qw: u64, dxgi: u64, adapter_ram: u64) -> u64 {
    if qw > 0 {
        return qw;
    }
    if dxgi > 0 {
        return dxgi;
    }
    adapter_ram
}

/// Virtual and remote adapters are not the player's GPU.
pub fn is_skipped_adapter(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("parsec")
        || n.contains("basic render")
        || n.contains("basic display")
        || n.contains("microsoft basic")
        || n.contains("virtual display")
        || n.contains("remote display")
}

fn looks_dedicated(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("basic render") || n.contains("microsoft basic") {
        return false;
    }
    if n.contains("nvidia") || n.contains("geforce") || n.contains("rtx") || n.contains("gtx") {
        return true;
    }
    if n.contains("radeon") || n.contains("amd ") || n.contains("rx ") {
        return !n.contains("graphics") || n.contains("rx ");
    }
    if n.contains("intel arc") {
        return true;
    }
    false
}

fn prefer(a: &GpuInfo, b: &GpuInfo) -> GpuInfo {
    if a.dedicated && !b.dedicated {
        return a.clone();
    }
    if b.dedicated && !a.dedicated {
        return b.clone();
    }
    if a.vram_gb >= b.vram_gb {
        a.clone()
    } else {
        b.clone()
    }
}

/// Parse `lspci -v` (or `lspci -vnn`) VGA / 3D blocks.
pub fn parse_lspci(output: &str) -> Option<GpuInfo> {
    let mut best: Option<GpuInfo> = None;
    let mut name = String::new();
    let mut vram_gb = 0.0;
    let mut in_gpu = false;

    let flush =
        |best: &mut Option<GpuInfo>, name: &mut String, vram_gb: &mut f64, in_gpu: &mut bool| {
            if *in_gpu && !name.is_empty() {
                let info = GpuInfo {
                    dedicated: looks_dedicated(name),
                    name: name.clone(),
                    vram_gb: *vram_gb,
                };
                *best = Some(match best.take() {
                    Some(prev) => prefer(&prev, &info),
                    None => info,
                });
            }
            name.clear();
            *vram_gb = 0.0;
            *in_gpu = false;
        };

    for line in output.lines() {
        if !line.starts_with('\t') && !line.starts_with(' ') {
            flush(&mut best, &mut name, &mut vram_gb, &mut in_gpu);
            let lower = line.to_ascii_lowercase();
            if lower.contains("vga compatible controller") || lower.contains("3d controller") {
                in_gpu = true;
                if let Some((_, rest)) = line.split_once(": ") {
                    name = rest
                        .split(" (rev")
                        .next()
                        .unwrap_or(rest)
                        .trim()
                        .to_string();
                }
            }
            continue;
        }
        if !in_gpu {
            continue;
        }
        let trimmed = line.trim();
        if let Some(size) = parse_lspci_memory_size(trimmed) {
            if size > vram_gb {
                vram_gb = size;
            }
        }
    }
    flush(&mut best, &mut name, &mut vram_gb, &mut in_gpu);
    best
}

fn parse_lspci_memory_size(line: &str) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("memory at") || !lower.contains("[size=") {
        return None;
    }
    let start = lower.find("[size=")? + 6;
    let rest = &lower[start..];
    let end = rest.find(']')?;
    let token = &rest[..end];
    if let Some(num) = token.strip_suffix('g') {
        return num.parse::<f64>().ok();
    }
    if let Some(num) = token.strip_suffix('m') {
        return num.parse::<f64>().ok().map(mb_to_gb);
    }
    None
}

/// Parse `/sys/class/drm/card*/device/uevent` plus `mem_info_vram_total` bytes.
pub fn parse_sysfs_gpu(uevent: &str, vram_bytes: Option<u64>, vendor_id: &str) -> Option<GpuInfo> {
    let mut driver = String::new();
    for line in uevent.lines() {
        if let Some(value) = line.strip_prefix("DRIVER=") {
            driver = value.trim().to_string();
        }
    }
    if driver.is_empty() && vendor_id.is_empty() && vram_bytes.is_none() {
        return None;
    }
    let name = match vendor_id.to_ascii_lowercase().as_str() {
        "0x10de" | "10de" => format!("NVIDIA ({driver})"),
        "0x1002" | "1002" => format!("AMD ({driver})"),
        "0x8086" | "8086" => format!("Intel ({driver})"),
        _ if !driver.is_empty() => driver.clone(),
        _ => "Unknown GPU".into(),
    };
    Some(GpuInfo {
        dedicated: looks_dedicated(&name) || matches!(driver.as_str(), "nvidia" | "amdgpu"),
        vram_gb: vram_bytes.map(bytes_to_gb).unwrap_or(0.0),
        name,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmiAdapter {
    pub name: String,
    pub adapter_ram: u64,
    pub pnp_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryGpu {
    pub driver_desc: String,
    pub matching_device_id: String,
    pub qw_memory_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxgiAdapter {
    pub name: String,
    pub dedicated_video_memory: u64,
}

/// Parse `Get-CimInstance Win32_VideoController | ConvertTo-Json`.
pub fn parse_wmi_json(text: &str) -> Option<GpuInfo> {
    let (wmi, _) = parse_windows_probe_json(text);
    select_windows_gpu(&wmi, &[], &[])
}

pub fn parse_windows_probe_json(text: &str) -> (Vec<WmiAdapter>, Vec<RegistryGpu>) {
    let text = text.trim().trim_start_matches('\u{feff}');
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return (Vec::new(), Vec::new());
    };
    if value.is_array() || value.get("Name").is_some() {
        return (parse_wmi_value(&value), Vec::new());
    }
    let wmi = value.get("wmi").map(parse_wmi_value).unwrap_or_default();
    let registry = value
        .get("registry")
        .map(parse_registry_value)
        .unwrap_or_default();
    (wmi, registry)
}

fn parse_wmi_value(value: &serde_json::Value) -> Vec<WmiAdapter> {
    let rows = json_rows(value);
    rows.iter()
        .filter_map(|row| {
            let name = json_str(row, &["Name", "name"]);
            if name.is_empty() {
                return None;
            }
            Some(WmiAdapter {
                name,
                adapter_ram: json_u64(row.get("AdapterRAM").or_else(|| row.get("adapterRAM"))),
                pnp_device_id: json_str(row, &["PNPDeviceID", "pnpDeviceId", "PnpDeviceId"]),
            })
        })
        .collect()
}

fn parse_registry_value(value: &serde_json::Value) -> Vec<RegistryGpu> {
    json_rows(value)
        .iter()
        .filter_map(|row| {
            let driver_desc = json_str(row, &["DriverDesc", "driverDesc"]);
            let matching_device_id = json_str(row, &["MatchingDeviceId", "matchingDeviceId"]);
            if driver_desc.is_empty() && matching_device_id.is_empty() {
                return None;
            }
            Some(RegistryGpu {
                driver_desc,
                matching_device_id,
                qw_memory_size: json_u64(
                    row.get("qwMemorySize")
                        .or_else(|| row.get("QwMemorySize"))
                        .or_else(|| row.get("HardwareInformation.qwMemorySize")),
                ),
            })
        })
        .collect()
}

fn json_rows(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(rows) = value.as_array() {
        return rows.clone();
    }
    if value.is_object() {
        return vec![value.clone()];
    }
    Vec::new()
}

fn json_str(row: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(text) = row.get(*key).and_then(|v| v.as_str()) {
            return text.trim().to_string();
        }
    }
    String::new()
}

fn json_u64(value: Option<&serde_json::Value>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    match value {
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                return value;
            }
            if let Some(value) = number.as_i64() {
                return if value < 0 {
                    value as u32 as u64
                } else {
                    value as u64
                };
            }
            number
                .as_f64()
                .filter(|value| *value > 0.0)
                .map(|value| value as u64)
                .unwrap_or(0)
        }
        serde_json::Value::String(text) => {
            let text = text.trim();
            if let Ok(value) = text.parse::<u64>() {
                return value;
            }
            if let Ok(value) = text.parse::<i64>() {
                return if value < 0 {
                    value as u32 as u64
                } else {
                    value as u64
                };
            }
            0
        }
        _ => 0,
    }
}

fn names_match(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    !left.is_empty() && left.eq_ignore_ascii_case(right)
}

fn device_ids_match(matching_device_id: &str, pnp_device_id: &str) -> bool {
    let matching = matching_device_id.trim().trim_end_matches(['\\', '/']);
    let pnp = pnp_device_id.trim();
    if matching.len() < 8 || pnp.is_empty() {
        return false;
    }
    pnp.to_ascii_lowercase()
        .starts_with(&matching.to_ascii_lowercase())
}

struct GpuCandidate {
    name: String,
    pnp: String,
    qw: u64,
    dxgi: u64,
    adapter_ram: u64,
}

/// Pick the adapter with the most dedicated memory.
/// Skips Microsoft Basic Render/Display, Parsec, other virtual or remote adapters,
/// and anything whose resolved memory is 0.
pub fn select_windows_gpu(
    wmi: &[WmiAdapter],
    registry: &[RegistryGpu],
    dxgi: &[DxgiAdapter],
) -> Option<GpuInfo> {
    let mut candidates: Vec<GpuCandidate> = Vec::new();
    for adapter in wmi {
        let name = adapter.name.trim();
        if name.is_empty() || is_skipped_adapter(name) {
            continue;
        }
        candidates.push(GpuCandidate {
            name: name.to_string(),
            pnp: adapter.pnp_device_id.clone(),
            qw: 0,
            dxgi: 0,
            adapter_ram: adapter.adapter_ram,
        });
    }
    for entry in registry {
        let desc = entry.driver_desc.trim();
        if is_skipped_adapter(desc) {
            continue;
        }
        if let Some(slot) = candidates.iter_mut().find(|candidate| {
            names_match(&candidate.name, desc)
                || device_ids_match(&entry.matching_device_id, &candidate.pnp)
        }) {
            if entry.qw_memory_size > slot.qw {
                slot.qw = entry.qw_memory_size;
            }
        } else if !desc.is_empty() {
            candidates.push(GpuCandidate {
                name: desc.to_string(),
                pnp: entry.matching_device_id.clone(),
                qw: entry.qw_memory_size,
                dxgi: 0,
                adapter_ram: 0,
            });
        }
    }
    for adapter in dxgi {
        let name = adapter.name.trim();
        if name.is_empty() || is_skipped_adapter(name) {
            continue;
        }
        if let Some(slot) = candidates
            .iter_mut()
            .find(|candidate| names_match(&candidate.name, name))
        {
            if adapter.dedicated_video_memory > slot.dxgi {
                slot.dxgi = adapter.dedicated_video_memory;
            }
        } else {
            candidates.push(GpuCandidate {
                name: name.to_string(),
                pnp: String::new(),
                qw: 0,
                dxgi: adapter.dedicated_video_memory,
                adapter_ram: 0,
            });
        }
    }

    let mut best: Option<(u64, GpuInfo)> = None;
    for candidate in candidates {
        let bytes = resolve_vram_bytes(candidate.qw, candidate.dxgi, candidate.adapter_ram);
        if bytes == 0 || is_skipped_adapter(&candidate.name) {
            continue;
        }
        let info = GpuInfo {
            dedicated: looks_dedicated(&candidate.name),
            name: candidate.name,
            vram_gb: bytes_to_gb(bytes),
        };
        best = Some(match best {
            None => (bytes, info),
            Some((prev_bytes, prev)) => {
                if bytes > prev_bytes {
                    (bytes, info)
                } else if bytes < prev_bytes {
                    (prev_bytes, prev)
                } else {
                    (prev_bytes, prefer(&prev, &info))
                }
            }
        });
    }
    best.map(|(_, info)| info)
}

/// Parse `system_profiler SPDisplaysDataType` text.
pub fn parse_system_profiler(output: &str) -> Option<GpuInfo> {
    let mut best: Option<GpuInfo> = None;
    let mut name = String::new();
    let mut vram_gb = 0.0;
    let mut dedicated = false;

    let flush = |best: &mut Option<GpuInfo>, name: &mut String, vram_gb: f64, dedicated: bool| {
        if name.is_empty() {
            return;
        }
        let info = GpuInfo {
            name: name.clone(),
            vram_gb,
            dedicated,
        };
        *best = Some(match best.take() {
            Some(prev) => prefer(&prev, &info),
            None => info,
        });
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(model) = trimmed.strip_prefix("Chipset Model:") {
            flush(&mut best, &mut name, vram_gb, dedicated);
            name = model.trim().to_string();
            vram_gb = 0.0;
            dedicated = false;
            continue;
        }
        if name.is_empty() {
            continue;
        }
        if let Some(bus) = trimmed.strip_prefix("Bus:") {
            dedicated = bus.trim().eq_ignore_ascii_case("PCIe");
        }
        if let Some(rest) = trimmed
            .strip_prefix("VRAM (Total):")
            .or_else(|| trimmed.strip_prefix("VRAM (Dynamic, Max):"))
        {
            dedicated = dedicated || trimmed.contains("Total");
            vram_gb = parse_vram_token(rest.trim()).unwrap_or(vram_gb);
        }
    }
    flush(&mut best, &mut name, vram_gb, dedicated);
    best
}

fn parse_vram_token(token: &str) -> Option<f64> {
    let parts: Vec<&str> = token.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let num: f64 = parts[0].replace(',', "").parse().ok()?;
    match parts[1].to_ascii_lowercase().as_str() {
        "gb" => Some((num * 10.0).round() / 10.0),
        "mb" => Some(mb_to_gb(num)),
        _ => None,
    }
}

pub fn probe_gpu() -> GpuInfo {
    #[cfg(target_os = "windows")]
    {
        if let Some(info) = probe_windows() {
            return info;
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(info) = probe_macos() {
            return info;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(info) = probe_linux() {
            return info;
        }
    }
    GpuInfo::unknown()
}

#[cfg(target_os = "windows")]
fn probe_windows() -> Option<GpuInfo> {
    let (wmi, registry) = read_wmi_and_registry().unwrap_or_default();
    let dxgi = read_dxgi();
    select_windows_gpu(&wmi, &registry, &dxgi)
}

#[cfg(target_os = "windows")]
fn read_wmi_and_registry() -> Option<(Vec<WmiAdapter>, Vec<RegistryGpu>)> {
    let output = crate::process::command("powershell")
        .args(["-NoProfile", "-Command", WINDOWS_GPU_SCRIPT])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_windows_probe_json(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\NNNN`
/// `HardwareInformation.qwMemorySize` is the 64-bit framebuffer size.
#[cfg(target_os = "windows")]
const WINDOWS_GPU_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$wmi = @(Get-CimInstance Win32_VideoController | ForEach-Object {
  $ram = '0'
  if ($null -ne $_.AdapterRAM) { $ram = [string]([uint64]$_.AdapterRAM) }
  [PSCustomObject]@{
    Name = [string]$_.Name
    AdapterRAM = $ram
    PNPDeviceID = [string]$_.PNPDeviceID
  }
})
$base = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
$reg = @()
if (Test-Path -LiteralPath $base) {
  $reg = @(Get-ChildItem -LiteralPath $base | Where-Object { $_.PSChildName -match '^\d{4}$' } | ForEach-Object {
    $p = Get-ItemProperty -LiteralPath $_.PSPath
    $qw = $p.'HardwareInformation.qwMemorySize'
    $qwText = '0'
    if ($null -ne $qw) { $qwText = [string]([uint64]$qw) }
    [PSCustomObject]@{
      DriverDesc = [string]$p.DriverDesc
      MatchingDeviceId = [string]$p.MatchingDeviceId
      qwMemorySize = $qwText
    }
  })
}
[PSCustomObject]@{ wmi = $wmi; registry = $reg } | ConvertTo-Json -Depth 5 -Compress
"#;

#[cfg(target_os = "windows")]
fn read_dxgi() -> Vec<DxgiAdapter> {
    dxgi_probe::dedicated_adapters()
        .into_iter()
        .map(|(name, dedicated_video_memory)| DxgiAdapter {
            name,
            dedicated_video_memory,
        })
        .collect()
}

/// Small `dxgi.dll` probe. `DXGI_ADAPTER_DESC.DedicatedVideoMemory` is a `SIZE_T`,
/// so it is not stuck at 4 GiB the way `AdapterRAM` is.
#[cfg(target_os = "windows")]
mod dxgi_probe {
    use std::ffi::c_void;

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    // IID_IDXGIFactory {7b7166ec-21c7-44ae-b21a-c9ae321ae369}
    const IID_IDXGI_FACTORY: Guid = Guid {
        data1: 0x7b7166ec,
        data2: 0x21c7,
        data3: 0x44ae,
        data4: [0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69],
    };

    #[link(name = "dxgi")]
    extern "system" {
        fn CreateDXGIFactory(riid: *const Guid, factory: *mut *mut c_void) -> i32;
    }

    #[repr(C)]
    struct Luid {
        low: u32,
        high: i32,
    }

    #[repr(C)]
    struct AdapterDesc {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        subsys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        adapter_luid: Luid,
    }

    type HRESULT = i32;
    type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
    type EnumAdaptersFn = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT;
    type GetDescFn = unsafe extern "system" fn(*mut c_void, *mut AdapterDesc) -> HRESULT;

    #[repr(C)]
    #[allow(dead_code)]
    struct FactoryVtbl {
        query_interface: usize,
        add_ref: usize,
        release: ReleaseFn,
        set_private_data: usize,
        set_private_data_interface: usize,
        get_private_data: usize,
        get_parent: usize,
        enum_adapters: EnumAdaptersFn,
        make_window_association: usize,
        get_window_association: usize,
        create_swap_chain: usize,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct AdapterVtbl {
        query_interface: usize,
        add_ref: usize,
        release: ReleaseFn,
        set_private_data: usize,
        set_private_data_interface: usize,
        get_private_data: usize,
        get_parent: usize,
        enum_outputs: usize,
        get_desc: GetDescFn,
        check_interface_support: usize,
    }

    pub fn dedicated_adapters() -> Vec<(String, u64)> {
        unsafe {
            let mut factory: *mut c_void = std::ptr::null_mut();
            if CreateDXGIFactory(&IID_IDXGI_FACTORY, &mut factory) < 0 || factory.is_null() {
                return Vec::new();
            }
            let vtbl = *(factory as *const *const FactoryVtbl);
            let mut found = Vec::new();
            let mut index = 0u32;
            loop {
                if index > 32 {
                    break;
                }
                let mut adapter: *mut c_void = std::ptr::null_mut();
                let hr = ((*vtbl).enum_adapters)(factory, index, &mut adapter);
                if hr < 0 || adapter.is_null() {
                    break;
                }
                let adapter_vtbl = *(adapter as *const *const AdapterVtbl);
                let mut desc = std::mem::zeroed::<AdapterDesc>();
                if ((*adapter_vtbl).get_desc)(adapter, &mut desc) >= 0 {
                    let name = utf16_buf(&desc.description);
                    if !name.is_empty() {
                        found.push((name, desc.dedicated_video_memory as u64));
                    }
                }
                ((*adapter_vtbl).release)(adapter);
                index += 1;
            }
            ((*vtbl).release)(factory);
            found
        }
    }

    fn utf16_buf(buf: &[u16]) -> String {
        let end = buf.iter().position(|&unit| unit == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end]).trim().to_string()
    }
}

#[cfg(target_os = "macos")]
fn probe_macos() -> Option<GpuInfo> {
    let output = crate::process::command("system_profiler")
        .args(["SPDisplaysDataType"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_system_profiler(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "linux")]
fn probe_linux() -> Option<GpuInfo> {
    if let Some(from_sysfs) = probe_linux_sysfs() {
        if from_sysfs.vram_gb > 0.0 || from_sysfs.dedicated {
            return Some(from_sysfs);
        }
        if let Some(from_lspci) = probe_linux_lspci() {
            return Some(from_lspci);
        }
        return Some(from_sysfs);
    }
    probe_linux_lspci()
}

#[cfg(target_os = "linux")]
fn probe_linux_lspci() -> Option<GpuInfo> {
    let output = crate::process::command("lspci")
        .args(["-v"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_lspci(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "linux")]
fn probe_linux_sysfs() -> Option<GpuInfo> {
    let drm = std::path::Path::new("/sys/class/drm");
    let entries = std::fs::read_dir(drm).ok()?;
    let mut best: Option<GpuInfo> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let uevent = std::fs::read_to_string(device.join("uevent")).unwrap_or_default();
        let vendor = std::fs::read_to_string(device.join("vendor")).unwrap_or_default();
        let vram = std::fs::read_to_string(device.join("mem_info_vram_total"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        if let Some(info) = parse_sysfs_gpu(&uevent, vram, vendor.trim()) {
            best = Some(match best.take() {
                Some(prev) => prefer(&prev, &info),
                None => info,
            });
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_rtx_from_lspci() {
        let sample = r#"
00:02.0 VGA compatible controller: Intel Corporation UHD Graphics 620 (rev 07)
	Memory at c0000000 (64-bit, prefetchable) [size=256M]
01:00.0 VGA compatible controller: NVIDIA Corporation GA106 [GeForce RTX 3060] (rev a1)
	Memory at 4000000000 (64-bit, prefetchable) [size=8G]
"#;
        let gpu = parse_lspci(sample).unwrap();
        assert!(gpu.name.contains("RTX 3060"));
        assert_eq!(gpu.vram_gb, 8.0);
        assert!(gpu.dedicated);
    }

    #[test]
    fn parses_sysfs_amd() {
        let gpu = parse_sysfs_gpu(
            "DRIVER=amdgpu\nPCI_ID=1002:73DF\n",
            Some(8 * 1024 * 1024 * 1024),
            "0x1002",
        )
        .unwrap();
        assert!(gpu.name.contains("AMD"));
        assert_eq!(gpu.vram_gb, 8.0);
        assert!(gpu.dedicated);
    }

    #[test]
    fn parses_wmi_array() {
        let json = r#"[
            {"Name":"Microsoft Basic Render Driver","AdapterRAM":0},
            {"Name":"NVIDIA GeForce RTX 3060","AdapterRAM":12884901888}
        ]"#;
        let gpu = parse_wmi_json(json).unwrap();
        assert!(gpu.name.contains("RTX 3060"));
        assert!(gpu.dedicated);
        assert!(gpu.vram_gb >= 11.0);
    }

    #[test]
    fn parses_system_profiler_discrete_and_apple() {
        let discrete = r#"
    AMD Radeon Pro 5500M:
      Chipset Model: AMD Radeon Pro 5500M
      Bus: PCIe
      VRAM (Total): 8 GB
    Intel UHD Graphics 630:
      Chipset Model: Intel UHD Graphics 630
      Bus: Built-In
      VRAM (Dynamic, Max): 1536 MB
"#;
        let gpu = parse_system_profiler(discrete).unwrap();
        assert!(gpu.name.contains("5500M"));
        assert_eq!(gpu.vram_gb, 8.0);
        assert!(gpu.dedicated);

        let apple = r#"
    Apple M1:
      Chipset Model: Apple M1
      Bus: Built-In
      VRAM (Dynamic, Max): 8192 MB
"#;
        let m1 = parse_system_profiler(apple).unwrap();
        assert_eq!(m1.name, "Apple M1");
        assert_eq!(m1.vram_gb, 8.0);
        assert!(!m1.dedicated);
    }

    /// 15.984 GiB. A 16 GB card's QWORD is often a hair under 16; one-decimal GiB rounding is 16.0.
    const RX_9070_XT_QW: u64 = 17_163_091_968;

    fn rx_9070() -> (WmiAdapter, RegistryGpu) {
        (
            WmiAdapter {
                name: "AMD Radeon RX 9070 XT".into(),
                adapter_ram: ADAPTER_RAM_CAP_SENTINEL,
                pnp_device_id: r"PCI\VEN_1002&DEV_7550&SUBSYS_1234&REV_00\4&abc".into(),
            },
            RegistryGpu {
                driver_desc: "AMD Radeon RX 9070 XT".into(),
                matching_device_id: r"PCI\VEN_1002&DEV_7550&SUBSYS_1234".into(),
                qw_memory_size: RX_9070_XT_QW,
            },
        )
    }

    #[test]
    fn capped_adapter_ram_never_overrides_a_larger_value() {
        assert!(adapter_ram_is_capped(ADAPTER_RAM_CAP_SENTINEL));
        assert!(adapter_ram_is_capped(FOUR_GIB));
        assert!(adapter_ram_is_capped(u32::MAX as u64));
        assert_eq!(bytes_to_gb(ADAPTER_RAM_CAP_SENTINEL), 4.0);
        assert_eq!(bytes_to_gb(FOUR_GIB), 4.0);
        assert_eq!(bytes_to_gb(RX_9070_XT_QW), 16.0);
        assert_eq!(
            resolve_vram_bytes(RX_9070_XT_QW, 0, ADAPTER_RAM_CAP_SENTINEL),
            RX_9070_XT_QW
        );
        assert_eq!(
            resolve_vram_bytes(0, RX_9070_XT_QW, FOUR_GIB),
            RX_9070_XT_QW
        );
        assert_eq!(resolve_vram_bytes(0, 0, FOUR_GIB), FOUR_GIB);
        assert!(!adapter_ram_is_capped(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn reinhardt_card_reads_16gb_and_skips_parsec() {
        let (wmi, registry) = rx_9070();
        let gpu = select_windows_gpu(
            &[
                WmiAdapter {
                    name: "Microsoft Basic Render Driver".into(),
                    adapter_ram: 0,
                    pnp_device_id: r"ROOT\BASICRENDER\0000".into(),
                },
                WmiAdapter {
                    name: "Microsoft Basic Display Adapter".into(),
                    adapter_ram: 0,
                    pnp_device_id: r"ROOT\BASICDISPLAY\0000".into(),
                },
                WmiAdapter {
                    name: "Parsec Virtual Display Adapter".into(),
                    adapter_ram: 0,
                    pnp_device_id: r"ROOT\PARSEC\0000".into(),
                },
                wmi,
            ],
            &[
                RegistryGpu {
                    driver_desc: "Parsec Virtual Display Adapter".into(),
                    matching_device_id: r"ROOT\PARSEC".into(),
                    qw_memory_size: 0,
                },
                registry,
            ],
            &[DxgiAdapter {
                name: "AMD Radeon RX 9070 XT".into(),
                dedicated_video_memory: RX_9070_XT_QW,
            }],
        )
        .unwrap();
        assert_eq!(gpu.name, "AMD Radeon RX 9070 XT");
        assert_eq!(gpu.vram_gb, 16.0);
        assert!(gpu.dedicated);
    }

    #[test]
    fn parsec_with_reported_memory_is_still_skipped() {
        let gpu = select_windows_gpu(
            &[
                WmiAdapter {
                    name: "Parsec Virtual Display Adapter".into(),
                    adapter_ram: 8 * 1024 * 1024 * 1024,
                    pnp_device_id: r"ROOT\PARSEC\0000".into(),
                },
                WmiAdapter {
                    name: "NVIDIA GeForce GTX 1650".into(),
                    adapter_ram: 4 * 1024 * 1024 * 1024,
                    pnp_device_id: r"PCI\VEN_10DE&DEV_1F82\1".into(),
                },
            ],
            &[],
            &[],
        )
        .unwrap();
        assert!(gpu.name.contains("1650"));
        assert_eq!(gpu.vram_gb, 4.0);
    }

    #[test]
    fn qword_matches_by_device_id_when_the_driver_desc_differs() {
        let gpu = select_windows_gpu(
            &[WmiAdapter {
                name: "AMD Radeon RX 9070 XT".into(),
                adapter_ram: FOUR_GIB,
                pnp_device_id: r"PCI\VEN_1002&DEV_7550&SUBSYS_1234&REV_00\4&abc".into(),
            }],
            &[RegistryGpu {
                driver_desc: "AMD Radeon Graphics".into(),
                matching_device_id: r"pci\ven_1002&dev_7550&subsys_1234".into(),
                qw_memory_size: RX_9070_XT_QW,
            }],
            &[],
        )
        .unwrap();
        assert_eq!(gpu.vram_gb, 16.0);
        assert!(gpu.name.contains("9070"));
    }

    #[test]
    fn dxgi_wins_when_the_registry_value_is_missing() {
        let gpu = select_windows_gpu(
            &[WmiAdapter {
                name: "AMD Radeon RX 9070 XT".into(),
                adapter_ram: ADAPTER_RAM_CAP_SENTINEL,
                pnp_device_id: String::new(),
            }],
            &[],
            &[DxgiAdapter {
                name: "AMD Radeon RX 9070 XT".into(),
                dedicated_video_memory: RX_9070_XT_QW,
            }],
        )
        .unwrap();
        assert_eq!(gpu.vram_gb, 16.0);
    }

    #[test]
    fn probe_json_reads_qword_strings() {
        let json = r#"{
            "wmi":[{"Name":"AMD Radeon RX 9070 XT","AdapterRAM":"4293918720","PNPDeviceID":"PCI\\VEN_1002&DEV_7550\\1"}],
            "registry":[{"DriverDesc":"AMD Radeon RX 9070 XT","MatchingDeviceId":"PCI\\VEN_1002&DEV_7550","qwMemorySize":"17163091968"}]
        }"#;
        let (wmi, registry) = parse_windows_probe_json(json);
        let gpu = select_windows_gpu(&wmi, &registry, &[]).unwrap();
        assert_eq!(gpu.vram_gb, 16.0);
    }
}
