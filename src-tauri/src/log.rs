use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::errors::AppError;
use crate::gpu::probe_gpu;
use crate::scan::probe_hardware;
use crate::state::{installer_dir, load_state};
use crate::updater::APP_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemSummary {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub cpu: String,
    pub cores: u32,
    pub ram_gb: f64,
    pub gpu: String,
    pub vram_gb: f64,
    pub dedicated_gpu: bool,
    pub minecraft: String,
    #[serde(default)]
    pub game_dir: Option<String>,
    #[serde(default)]
    pub manifest_version: Option<String>,
    #[serde(default)]
    pub manifest_source: Option<String>,
}

pub fn logs_dir(game_dir: &Path) -> PathBuf {
    installer_dir(game_dir).join("logs")
}

pub fn append_install_log(game_dir: &Path, event: &serde_json::Value) -> Result<(), AppError> {
    let dir = logs_dir(game_dir);
    fs::create_dir_all(&dir)?;
    let path = dir.join("install.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{event}")?;
    Ok(())
}

pub fn build_summary(
    game_dir: Option<&Path>,
    minecraft: &str,
    manifest_version: Option<&str>,
    manifest_source: Option<&str>,
) -> SystemSummary {
    let hw = probe_hardware();
    let gpu = if hw.gpu.starts_with("Unknown") {
        let probed = probe_gpu();
        (probed.name, probed.vram_gb, probed.dedicated)
    } else {
        (hw.gpu.clone(), hw.vram_gb, hw.dedicated_gpu)
    };
    SystemSummary {
        app_version: APP_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu: hw.cpu,
        cores: hw.cores,
        ram_gb: hw.ram_gb,
        gpu: gpu.0,
        vram_gb: gpu.1,
        dedicated_gpu: gpu.2,
        minecraft: minecraft.to_string(),
        game_dir: game_dir.map(|p| p.display().to_string()),
        manifest_version: manifest_version.map(ToString::to_string),
        manifest_source: manifest_source.map(ToString::to_string),
    }
}

pub fn diagnostics_text(summary: &SystemSummary, extra: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("Arnold Client Mods diagnostics\n");
    out.push_str(&format!("app: {}\n", summary.app_version));
    out.push_str(&format!("os: {} {}\n", summary.os, summary.arch));
    out.push_str(&format!("cpu: {} ({} cores)\n", summary.cpu, summary.cores));
    out.push_str(&format!("ram: {} GB\n", summary.ram_gb));
    out.push_str(&format!(
        "gpu: {} ({} GB, dedicated={})\n",
        summary.gpu, summary.vram_gb, summary.dedicated_gpu
    ));
    out.push_str(&format!("minecraft: {}\n", summary.minecraft));
    if let Some(dir) = &summary.game_dir {
        out.push_str(&format!("gameDir: {dir}\n"));
    }
    if let Some(ver) = &summary.manifest_version {
        out.push_str(&format!("manifest: {ver}\n"));
    }
    if let Some(source) = &summary.manifest_source {
        out.push_str(&format!("manifestSource: {source}\n"));
    }
    if let Some(extra) = extra {
        out.push('\n');
        out.push_str(extra);
        if !extra.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

pub fn export_log_zip(
    game_dir: Option<&Path>,
    dest_dir: Option<&Path>,
    summary: &SystemSummary,
) -> Result<PathBuf, AppError> {
    let stamp = crate::state::now_stamp();
    let exports = match (dest_dir, game_dir) {
        (Some(dir), _) => dir.to_path_buf(),
        (None, Some(game)) => installer_dir(game).join("exports"),
        (None, None) => dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("arnold-client-mods")
            .join("exports"),
    };
    fs::create_dir_all(&exports)?;
    let zip_path = exports.join(format!("arnold-diagnostics-{stamp}.zip"));
    let file = fs::File::create(&zip_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("system-summary.json", options)?;
    zip.write_all(&serde_json::to_vec_pretty(summary)?)?;

    zip.start_file("diagnostics.txt", options)?;
    zip.write_all(diagnostics_text(summary, None).as_bytes())?;

    if let Some(game) = game_dir {
        if let Some(state) = load_state(game)? {
            zip.start_file("state.json", options)?;
            zip.write_all(&serde_json::to_vec_pretty(&state)?)?;
        }
        let log_path = logs_dir(game).join("install.jsonl");
        if log_path.is_file() {
            zip.start_file("logs/install.jsonl", options)?;
            zip.write_all(&fs::read(log_path)?)?;
        }
    }

    zip.finish()?;
    Ok(zip_path)
}

pub fn export_log(game_dir: Option<&Path>) -> Result<PathBuf, AppError> {
    let summary = build_summary(game_dir, "26.3", None, None);
    export_install_log(game_dir, None, &summary, &local_export_stamp())
}

pub fn installer_logs_dir_from(documents: &Path) -> PathBuf {
    documents.join("Arnold Installer").join("logs")
}

pub fn installer_logs_dir() -> PathBuf {
    let documents = dirs::document_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Documents")
    });
    installer_logs_dir_from(&documents)
}

pub fn format_export_stamp(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}-{hour:02}{minute:02}")
}

pub fn export_log_filename(stamp: &str) -> String {
    format!("Arnold-install-log-{stamp}.txt")
}

pub fn local_export_stamp() -> String {
    let now = chrono::Local::now();
    format_export_stamp(now.year(), now.month(), now.day(), now.hour(), now.minute())
}

pub fn export_install_log(
    game_dir: Option<&Path>,
    dest_dir: Option<&Path>,
    summary: &SystemSummary,
    stamp: &str,
) -> Result<PathBuf, AppError> {
    let exports = dest_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(installer_logs_dir);
    fs::create_dir_all(&exports).map_err(|err| log_export_error(&exports, &err))?;
    let path = exports.join(export_log_filename(stamp));
    let body = export_log_text(game_dir, summary)?;
    fs::write(&path, body).map_err(|err| log_export_error(&path, &err))?;
    Ok(path)
}

fn export_log_text(game_dir: Option<&Path>, summary: &SystemSummary) -> Result<String, AppError> {
    let mut text = diagnostics_text(summary, None);
    if let Some(game) = game_dir {
        if let Some(state) = load_state(game)? {
            text.push_str("\n--- state.json ---\n");
            text.push_str(&serde_json::to_string_pretty(&state).map_err(|err| {
                AppError::State(format!("could not write state into the log: {err}"))
            })?);
            text.push('\n');
        }
        let log_path = logs_dir(game).join("install.jsonl");
        if log_path.is_file() {
            text.push_str("\n--- logs/install.jsonl ---\n");
            text.push_str(&fs::read_to_string(log_path)?);
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    Ok(text)
}

fn log_export_error(path: &Path, err: &std::io::Error) -> AppError {
    AppError::Other {
        code: "E-LOG-EXPORT".into(),
        title: "The install log could not be saved".into(),
        hint: format!(
            "Could not write {}. Check that your Documents folder is available, then try Export log again.",
            path.display()
        ),
        detail: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{record_file, save_state, InstalledState};
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn zip_contains_summary_state_and_logs_only() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "2026.11.1");
        record_file(&mut state, "mods/arnold-snowfall-1.3.3.jar", "abc", "snow");
        save_state(game, &state).unwrap();
        append_install_log(game, &serde_json::json!({"step":"done"})).unwrap();
        fs::write(game.join("secrets.txt"), b"do-not-pack").unwrap();
        let summary = SystemSummary {
            app_version: "3.0.0".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu: "Test CPU".into(),
            cores: 8,
            ram_gb: 16.0,
            gpu: "Test GPU".into(),
            vram_gb: 8.0,
            dedicated_gpu: true,
            minecraft: "26.3".into(),
            game_dir: Some(game.display().to_string()),
            manifest_version: Some("2026.11.01-1".into()),
            manifest_source: Some("bundled".into()),
        };
        let dest = tmp.path().join("out");
        let zip_path = export_log_zip(Some(game), Some(&dest), &summary).unwrap();
        let mut archive = ZipArchive::new(fs::File::open(&zip_path).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"system-summary.json".to_string()));
        assert!(names.contains(&"state.json".to_string()));
        assert!(names.contains(&"logs/install.jsonl".to_string()));
        assert!(names.contains(&"diagnostics.txt".to_string()));
        assert!(!names.iter().any(|n| n.contains("secrets")));
        let mut state_file = archive.by_name("state.json").unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut state_file, &mut text).unwrap();
        assert!(text.contains("arnold-snowfall-1.3.3.jar"));
    }

    #[test]
    fn diagnostics_text_includes_what_to_send() {
        let summary = SystemSummary {
            app_version: "3.0.0".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu: "CPU".into(),
            cores: 4,
            ram_gb: 8.0,
            gpu: "GPU".into(),
            vram_gb: 2.0,
            dedicated_gpu: true,
            minecraft: "26.3".into(),
            game_dir: Some("/home/sam/.minecraft".into()),
            manifest_version: Some("1".into()),
            manifest_source: Some("cache".into()),
        };
        let text = diagnostics_text(&summary, Some("E-HASH-MISMATCH"));
        assert!(text.contains("/home/sam/.minecraft"));
        assert!(text.contains("E-HASH-MISMATCH"));
        assert!(text.contains("3.0.0"));
    }

    #[test]
    fn export_filename_is_dated_under_documents() {
        assert_eq!(
            export_log_filename(&format_export_stamp(2026, 11, 2, 19, 15)),
            "Arnold-install-log-2026-11-02-1915.txt"
        );
        let docs = Path::new(r"C:\Users\Sam\Documents");
        assert_eq!(
            installer_logs_dir_from(docs),
            docs.join("Arnold Installer").join("logs")
        );
    }

    #[test]
    fn text_export_contains_summary_state_and_logs_only() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "2026.11.1");
        record_file(&mut state, "mods/arnold-snowfall-1.3.3.jar", "abc", "snow");
        save_state(game, &state).unwrap();
        append_install_log(game, &serde_json::json!({"step":"done"})).unwrap();
        fs::write(game.join("secrets.txt"), b"do-not-pack").unwrap();
        let summary = SystemSummary {
            app_version: "3.0.0".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            cpu: "Test CPU".into(),
            cores: 20,
            ram_gb: 31.8,
            gpu: "AMD Radeon RX 9070 XT".into(),
            vram_gb: 16.0,
            dedicated_gpu: true,
            minecraft: "26.3".into(),
            game_dir: Some(game.display().to_string()),
            manifest_version: Some("2026.11.01-1".into()),
            manifest_source: Some("local".into()),
        };
        let dest = tmp.path().join("out");
        let path = export_install_log(
            Some(game),
            Some(&dest),
            &summary,
            &format_export_stamp(2026, 11, 2, 19, 15),
        )
        .unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "Arnold-install-log-2026-11-02-1915.txt"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("arnold-snowfall-1.3.3.jar"));
        assert!(text.contains("\"step\":\"done\"") || text.contains("step"));
        assert!(text.contains("RX 9070 XT"));
        assert!(!text.contains("do-not-pack"));
        assert!(!text.contains("secrets.txt"));
    }

    #[test]
    fn export_error_names_the_documents_folder() {
        let err = log_export_error(
            Path::new(r"C:\Users\Sam\Documents\Arnold Installer\logs"),
            &std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        let payload = err.to_payload();
        assert_eq!(payload.code, "E-LOG-EXPORT");
        assert!(payload.title.contains("could not be saved"));
        assert!(payload.hint.contains("Documents"));
    }
}
