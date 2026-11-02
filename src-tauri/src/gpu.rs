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

/// Parse `Get-CimInstance Win32_VideoController | ConvertTo-Json`.
pub fn parse_wmi_json(text: &str) -> Option<GpuInfo> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let adapters = if value.is_array() {
        value.as_array()?.clone()
    } else {
        vec![value]
    };
    let mut best: Option<GpuInfo> = None;
    for adapter in adapters {
        let name = adapter
            .get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() || name.to_ascii_lowercase().contains("basic render") {
            continue;
        }
        let ram = adapter
            .get("AdapterRAM")
            .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|n| n as u64)))
            .unwrap_or(0);
        let info = GpuInfo {
            dedicated: looks_dedicated(&name),
            vram_gb: if ram > 0 { bytes_to_gb(ram) } else { 0.0 },
            name,
        };
        best = Some(match best.take() {
            Some(prev) => prefer(&prev, &info),
            None => info,
        });
    }
    best
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
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ConvertTo-Json",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_wmi_json(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "macos")]
fn probe_macos() -> Option<GpuInfo> {
    let output = std::process::Command::new("system_profiler")
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
    let output = std::process::Command::new("lspci")
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
}
