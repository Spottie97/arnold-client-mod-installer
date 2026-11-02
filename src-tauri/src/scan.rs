use serde::{Deserialize, Serialize};

use crate::gpu::probe_gpu;
use crate::manifest::{HardwareThresholds, RecommendConfig};
use crate::quality::{recommend_quality, QualityLevel};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub cpu: String,
    pub cores: u32,
    pub ram_gb: f64,
    pub gpu: String,
    pub vram_gb: f64,
    pub dedicated_gpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub kit: String,
    pub snow_lite: bool,
    pub quality: QualityLevel,
    pub recommend_ocean: bool,
    pub reasons: Vec<String>,
    pub hardware: Hardware,
    pub warn_low_ram: bool,
}

fn meets(hw: &Hardware, t: &HardwareThresholds) -> bool {
    if t.dedicated_gpu.unwrap_or(false) && !hw.dedicated_gpu {
        return false;
    }
    if let Some(vram) = t.vram_gb {
        if hw.vram_gb + f64::EPSILON < vram {
            return false;
        }
    }
    if let Some(ram) = t.ram_gb {
        if hw.ram_gb + f64::EPSILON < ram {
            return false;
        }
    }
    if let Some(cores) = t.cores {
        if hw.cores < cores {
            return false;
        }
    }
    true
}

fn default_recommend() -> RecommendConfig {
    RecommendConfig {
        full: Some(HardwareThresholds {
            dedicated_gpu: Some(true),
            vram_gb: Some(4.0),
            ram_gb: Some(16.0),
            cores: Some(4),
        }),
        mid_snow_full: Some(HardwareThresholds {
            dedicated_gpu: Some(true),
            vram_gb: Some(2.0),
            ram_gb: Some(8.0),
            cores: Some(4),
        }),
        warn_ram_below_gb: Some(8.0),
    }
}

/// Pure function. Thresholds come from the manifest; 2.3.4 defaults are the fallback.
pub fn recommend(hw: &Hardware, rec: Option<&RecommendConfig>) -> Recommendation {
    let rec = rec.cloned().unwrap_or_else(default_recommend);
    let warn_low_ram = rec
        .warn_ram_below_gb
        .is_some_and(|limit| hw.ram_gb + f64::EPSILON < limit);

    let mut reasons = Vec::new();
    if hw.dedicated_gpu {
        reasons.push(format!(
            "Dedicated GPU with {} GB VRAM",
            trim_num(hw.vram_gb)
        ));
    } else {
        reasons.push(format!("Integrated / unknown GPU ({})", hw.gpu));
    }
    reasons.push(format!("{} GB RAM", trim_num(hw.ram_gb)));
    reasons.push(format!("{} cores", hw.cores));

    let (kit, snow_lite) = if rec.full.as_ref().is_some_and(|t| meets(hw, t)) {
        reasons.push("Shaders: yes".into());
        ("full".to_string(), false)
    } else if rec.mid_snow_full.as_ref().is_some_and(|t| meets(hw, t)) {
        reasons.push("Shaders: no — this PC is happier on Sodium alone".into());
        ("basic".to_string(), false)
    } else {
        reasons.push("Shaders: no — lighter snow keeps FPS up".into());
        ("basic".to_string(), true)
    };
    let quality = recommend_quality(hw);
    reasons.push(format!("Quality: {}", quality.as_str()));
    reasons.push("Sodium: yes — it raises FPS on every PC".into());

    Recommendation {
        kit,
        snow_lite,
        quality,
        recommend_ocean: quality.recommend_ocean(),
        reasons,
        hardware: hw.clone(),
        warn_low_ram,
    }
}

fn trim_num(n: f64) -> String {
    if (n - n.round()).abs() < 0.05 {
        format!("{}", n.round() as i64)
    } else {
        format!("{n:.1}")
    }
}

/// RAM and CPU from sysinfo. GPU/VRAM from the live OS probe (DXGI/WMI, system_profiler, lspci/sysfs).
pub fn probe_hardware() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let cores = sys.cpus().len() as u32;
    let ram_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let cpu = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown CPU".into());
    let gpu = probe_gpu();
    Hardware {
        cpu,
        cores,
        ram_gb: (ram_gb * 10.0).round() / 10.0,
        gpu: gpu.name,
        vram_gb: gpu.vram_gb,
        dedicated_gpu: gpu.dedicated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::load_bundled_manifest;

    #[test]
    fn fixtures_match_expected_kits() {
        let manifest = load_bundled_manifest().unwrap();
        let rec = manifest.recommend.as_ref();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/hw");
        let mut seen = 0usize;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let hw = Hardware {
                cpu: v["cpu"].as_str().unwrap().into(),
                cores: v["cores"].as_u64().unwrap() as u32,
                ram_gb: v["ramGB"].as_f64().unwrap(),
                gpu: v["gpu"].as_str().unwrap().into(),
                vram_gb: v["vramGB"].as_f64().unwrap(),
                dedicated_gpu: v["dedicatedGpu"].as_bool().unwrap(),
            };
            let got = recommend(&hw, rec);
            assert_eq!(
                got.kit,
                v["expectKit"].as_str().unwrap(),
                "{}",
                path.display()
            );
            assert_eq!(
                got.snow_lite,
                v["expectSnowLite"].as_bool().unwrap(),
                "{}",
                path.display()
            );
            seen += 1;
        }
        assert!(seen >= 7, "expected hardware fixtures, got {seen}");
    }

    #[test]
    fn live_probe_feeds_recommend() {
        let hw = probe_hardware();
        assert!(hw.cores >= 1);
        assert!(hw.ram_gb >= 0.0);
        let rec = recommend(&hw, None);
        assert!(rec.kit == "full" || rec.kit == "basic");
    }

    #[test]
    fn corrected_vram_reaches_recommend_for_the_9070_xt() {
        use crate::gpu::{
            select_windows_gpu, DxgiAdapter, RegistryGpu, WmiAdapter, ADAPTER_RAM_CAP_SENTINEL,
        };
        use crate::quality::{recommend_quality, QualityLevel};

        let qw = 17_163_091_968u64;
        let gpu = select_windows_gpu(
            &[
                WmiAdapter {
                    name: "Parsec Virtual Display Adapter".into(),
                    adapter_ram: 0,
                    pnp_device_id: r"ROOT\PARSEC\0000".into(),
                },
                WmiAdapter {
                    name: "AMD Radeon RX 9070 XT".into(),
                    adapter_ram: ADAPTER_RAM_CAP_SENTINEL,
                    pnp_device_id: r"PCI\VEN_1002&DEV_7550&SUBSYS_1234\4".into(),
                },
            ],
            &[RegistryGpu {
                driver_desc: "AMD Radeon RX 9070 XT".into(),
                matching_device_id: r"PCI\VEN_1002&DEV_7550&SUBSYS_1234".into(),
                qw_memory_size: qw,
            }],
            &[DxgiAdapter {
                name: "AMD Radeon RX 9070 XT".into(),
                dedicated_video_memory: qw,
            }],
        )
        .unwrap();
        assert_eq!(gpu.vram_gb, 16.0);
        let manifest = load_bundled_manifest().unwrap();
        let hw = Hardware {
            cpu: "13th Gen Intel(R) Core(TM) i5-13600KF".into(),
            cores: 20,
            ram_gb: 31.8,
            gpu: gpu.name,
            vram_gb: gpu.vram_gb,
            dedicated_gpu: gpu.dedicated,
        };
        let rec = recommend(&hw, manifest.recommend.as_ref());
        assert_eq!(rec.kit, "full");
        assert!(!rec.snow_lite);
        assert!(rec.recommend_ocean);
        assert_eq!(rec.quality, QualityLevel::Ultra);
        assert_eq!(recommend_quality(&hw), QualityLevel::Ultra);
        assert!(rec.reasons.iter().any(|reason| reason.contains("16 GB")));

        let capped = Hardware {
            vram_gb: 4.0,
            ..hw.clone()
        };
        let old = recommend(&capped, manifest.recommend.as_ref());
        assert_eq!(old.kit, "full");
        assert!(!old.snow_lite);
        assert_eq!(old.quality, QualityLevel::Ultra);
        assert_eq!(recommend_quality(&capped), QualityLevel::Ultra);
        assert!(old.reasons.iter().any(|reason| reason.contains("4 GB")));
    }
}
