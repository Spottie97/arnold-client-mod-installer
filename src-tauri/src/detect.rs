use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    Macos,
    Linux,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub name: String,
    pub cmd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FabricStatus {
    pub installed: bool,
    pub profile_id: Option<String>,
    pub will_install: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub game_dir: Option<PathBuf>,
    pub candidates: Vec<PathBuf>,
    pub minecraft_version: String,
    pub minecraft_version_installed: bool,
    pub fabric: FabricStatus,
    pub game_running: bool,
    pub imported_from_legacy: bool,
    #[serde(default)]
    pub pack_id: String,
    #[serde(default)]
    pub pack_dir: Option<PathBuf>,
    #[serde(default)]
    pub launcher_root: PathBuf,
    #[serde(default)]
    pub folder_mode: String,
    #[serde(default)]
    pub needs_folder_choice: Option<crate::pack_dirs::LegacyInstall>,
    #[serde(default)]
    pub launchers: crate::launcher::LauncherReport,
    /// When false, a missing vanilla jar is not an install prerequisite.
    #[serde(default)]
    pub require_vanilla: bool,
}

pub fn apply_chosen_vanilla(detection: &mut Detection, roots: &[PathBuf], require_vanilla: bool) {
    detection.require_vanilla = require_vanilla;
    if roots.is_empty() {
        return;
    }
    let version = detection.minecraft_version.clone();
    if let Some(root) = roots
        .iter()
        .find(|root| !vanilla_jar_exists(root, &version))
    {
        detection.minecraft_version_installed = false;
        detection.game_dir = Some(root.clone());
    } else {
        detection.minecraft_version_installed = true;
    }
}

pub fn current_os() -> HostOs {
    if cfg!(target_os = "windows") {
        HostOs::Windows
    } else if cfg!(target_os = "macos") {
        HostOs::Macos
    } else {
        HostOs::Linux
    }
}

pub fn candidate_dirs(home: &Path, os: HostOs) -> Vec<PathBuf> {
    match os {
        HostOs::Windows => vec![home.join("AppData").join("Roaming").join(".minecraft")],
        HostOs::Macos => vec![home
            .join("Library")
            .join("Application Support")
            .join("minecraft")],
        HostOs::Linux => vec![
            home.join(".minecraft"),
            home.join(".var")
                .join("app")
                .join("com.mojang.Minecraft")
                .join(".minecraft"),
        ],
    }
}

pub fn default_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.extend(candidate_dirs(&home, current_os()));
    }
    if current_os() == HostOs::Windows {
        if let Some(roaming) = dirs::config_dir() {
            let classic = roaming.join(".minecraft");
            if !out.contains(&classic) {
                out.insert(0, classic);
            }
        }
    }
    out
}

pub fn vanilla_jar_path(game_dir: &Path, version: &str) -> PathBuf {
    game_dir
        .join("versions")
        .join(version)
        .join(format!("{version}.jar"))
}

pub fn vanilla_jar_exists(game_dir: &Path, version: &str) -> bool {
    vanilla_jar_path(game_dir, version).is_file()
}

pub fn fabric_profile_present(game_dir: &Path, loader_id: &str) -> bool {
    let profile_json = game_dir
        .join("versions")
        .join(loader_id)
        .join(format!("{loader_id}.json"));
    if profile_json.is_file() {
        return true;
    }
    let launcher = game_dir.join("launcher_profiles.json");
    if let Ok(text) = std::fs::read_to_string(launcher) {
        return text.contains(loader_id);
    }
    false
}

pub fn looks_like_legacy_install(game_dir: &Path) -> bool {
    let mods = game_dir.join("mods");
    let Ok(entries) = std::fs::read_dir(mods) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        name.starts_with("arnold-") && name.ends_with(".jar")
    })
}

pub fn process_looks_like_minecraft(name: &str, cmd: &str) -> bool {
    let name_l = name.to_ascii_lowercase();
    let cmd_l = cmd.to_ascii_lowercase();
    if name_l.contains("arnold") && (name_l.contains("mod") || name_l.contains("installer")) {
        return false;
    }
    if name_l.contains("minecraft") {
        return true;
    }
    let java = name_l == "java" || name_l == "javaw" || name_l.starts_with("java");
    if java {
        return cmd_l.contains("net.minecraft")
            || cmd_l.contains(".minecraft")
            || cmd_l.contains("minecraft.jar")
            || cmd_l.contains("fabric-loader")
            || (cmd_l.contains("minecraft") && !cmd_l.contains("arnold-client-mods"));
    }
    false
}

pub fn game_running_with(processes: &[ProcessInfo]) -> bool {
    processes
        .iter()
        .any(|p| process_looks_like_minecraft(&p.name, &p.cmd))
}

pub fn live_minecraft_processes() -> Vec<ProcessInfo> {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes()
        .values()
        .map(|proc| ProcessInfo {
            name: proc.name().to_string_lossy().into_owned(),
            cmd: proc
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
}

pub fn detect_at(
    game_dir: Option<&Path>,
    minecraft: &str,
    loader_id: &str,
    processes: &[ProcessInfo],
    search_roots: &[PathBuf],
) -> Detection {
    let mut candidates = search_roots.to_vec();
    if let Some(chosen) = game_dir {
        if !candidates.iter().any(|p| p == chosen) {
            candidates.insert(0, chosen.to_path_buf());
        }
    }
    let existing: Vec<PathBuf> = candidates.iter().filter(|p| p.is_dir()).cloned().collect();
    let selected = game_dir
        .map(Path::to_path_buf)
        .or_else(|| existing.first().cloned());
    let minecraft_version_installed = selected
        .as_deref()
        .is_some_and(|dir| vanilla_jar_exists(dir, minecraft));
    let fabric_installed = selected
        .as_deref()
        .is_some_and(|dir| fabric_profile_present(dir, loader_id));
    let imported = selected.as_deref().is_some_and(looks_like_legacy_install);
    Detection {
        game_dir: selected,
        candidates: existing,
        minecraft_version: minecraft.to_string(),
        minecraft_version_installed,
        fabric: FabricStatus {
            installed: fabric_installed,
            profile_id: fabric_installed.then(|| loader_id.to_string()),
            will_install: !fabric_installed,
        },
        game_running: game_running_with(processes),
        imported_from_legacy: imported,
        pack_id: String::new(),
        pack_dir: None,
        launcher_root: selected.clone().unwrap_or_default(),
        folder_mode: "own".into(),
        needs_folder_choice: None,
        launchers: crate::launcher::LauncherReport::default(),
        require_vanilla: false,
    }
}

pub fn require_minecraft(detection: &Detection) -> Result<(), AppError> {
    if detection.minecraft_version_installed {
        return Ok(());
    }
    let path = detection
        .game_dir
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no game folder)".into());
    Err(AppError::McMissing {
        version: detection.minecraft_version.clone(),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::snapshots_dir;
    use crate::log::logs_dir;
    use crate::manifest::FileDest;
    use crate::state::{installer_dir, state_path};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn linux_and_flatpak_candidates() {
        let home = Path::new("/home/sam");
        let dirs = candidate_dirs(home, HostOs::Linux);
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/sam/.minecraft"),
                PathBuf::from("/home/sam/.var/app/com.mojang.Minecraft/.minecraft"),
            ]
        );
    }

    #[test]
    fn windows_and_mac_candidates() {
        let win = candidate_dirs(Path::new("C:/Users/Sam"), HostOs::Windows);
        assert_eq!(
            win,
            vec![PathBuf::from("C:/Users/Sam/AppData/Roaming/.minecraft")]
        );
        let mac = candidate_dirs(Path::new("/Users/sam"), HostOs::Macos);
        assert_eq!(
            mac,
            vec![PathBuf::from(
                "/Users/sam/Library/Application Support/minecraft"
            )]
        );
    }

    #[test]
    fn detects_vanilla_jar_and_fabric() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join(".minecraft");
        fs::create_dir_all(game.join("versions/26.3")).unwrap();
        fs::write(game.join("versions/26.3/26.3.jar"), b"jar").unwrap();
        fs::create_dir_all(game.join("versions/fabric-loader-0.20.1-26.3")).unwrap();
        fs::write(
            game.join("versions/fabric-loader-0.20.1-26.3/fabric-loader-0.20.1-26.3.json"),
            b"{}",
        )
        .unwrap();
        let detection = detect_at(
            Some(&game),
            "26.3",
            "fabric-loader-0.20.1-26.3",
            &[],
            std::slice::from_ref(&game),
        );
        assert!(detection.minecraft_version_installed);
        assert!(detection.fabric.installed);
        require_minecraft(&detection).unwrap();
    }

    #[test]
    fn missing_jar_is_an_error() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join(".minecraft");
        fs::create_dir_all(&game).unwrap();
        let detection = detect_at(
            Some(&game),
            "26.3",
            "fabric-loader-0.20.1-26.3",
            &[],
            std::slice::from_ref(&game),
        );
        let err = require_minecraft(&detection).unwrap_err();
        assert_eq!(err.code(), "E-MC-MISSING");
    }

    #[test]
    fn user_picked_folder_is_preferred() {
        let tmp = tempdir().unwrap();
        let custom = tmp.path().join("games/mc");
        fs::create_dir_all(&custom).unwrap();
        let detection = detect_at(Some(&custom), "26.3", "loader", &[], &[]);
        assert_eq!(detection.game_dir.as_deref(), Some(custom.as_path()));
    }

    #[test]
    fn game_running_matcher() {
        assert!(process_looks_like_minecraft("Minecraft", ""));
        assert!(process_looks_like_minecraft(
            "javaw",
            "javaw -cp something net.minecraft.client.main.Main"
        ));
        assert!(process_looks_like_minecraft(
            "java",
            "/usr/bin/java -jar fabric-loader-0.20.1-26.3.jar"
        ));
        assert!(!process_looks_like_minecraft(
            "java",
            "/usr/bin/java -jar idea.jar"
        ));
        assert!(!process_looks_like_minecraft(
            "arnold-client-mods",
            "arnold-client-mods"
        ));
        assert!(game_running_with(&[ProcessInfo {
            name: "javaw".into(),
            cmd: "C:\\\\Program Files\\\\Java\\\\javaw.exe .minecraft\\\\natives".into(),
        }]));
        assert!(process_looks_like_minecraft(
            "java",
            "/Users/sam/Library/Application Support/minecraft/runtime/java-runtime-delta/mac-os-arm64/java-runtime-delta/jre.bundle/Contents/Home/bin/java -XstartOnFirstThread -cp ... net.minecraft.client.main.Main"
        ));
    }

    #[test]
    fn game_paths_resolve_under_detected_dir_per_os() {
        for (home, os, expected) in [
            (
                "/Users/sam",
                HostOs::Macos,
                "/Users/sam/Library/Application Support/minecraft",
            ),
            (
                "C:/Users/Sam",
                HostOs::Windows,
                "C:/Users/Sam/AppData/Roaming/.minecraft",
            ),
            ("/home/sam", HostOs::Linux, "/home/sam/.minecraft"),
        ] {
            let game = candidate_dirs(Path::new(home), os)[0].clone();
            assert_eq!(game, PathBuf::from(expected));
            for dest in [
                FileDest::Mods,
                FileDest::Shaderpacks,
                FileDest::Resourcepacks,
            ] {
                assert_eq!(
                    game.join(dest.folder()),
                    PathBuf::from(expected).join(dest.folder())
                );
            }
            assert_eq!(game.join("config"), PathBuf::from(expected).join("config"));
            assert_eq!(
                installer_dir(&game),
                PathBuf::from(expected).join("arnold-installer")
            );
            assert_eq!(
                snapshots_dir(&game),
                PathBuf::from(expected).join("arnold-installer/snapshots")
            );
            assert!(state_path(&game).starts_with(installer_dir(&game)));
            assert!(logs_dir(&game).starts_with(installer_dir(&game)));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn default_candidates_live_on_macos() {
        let home = dirs::home_dir().expect("home dir");
        let expected = home
            .join("Library")
            .join("Application Support")
            .join("minecraft");
        let candidates = default_candidates();
        assert_eq!(candidates[0], expected);
        assert!(candidates
            .iter()
            .all(|path| !path.to_string_lossy().contains("AppData")));
    }

    #[test]
    fn chosen_roots_override_a_missing_official_default_folder() {
        let tmp = tempdir().unwrap();
        let official = tmp.path().join("official");
        let custom = tmp.path().join("custom");
        fs::create_dir_all(&official).unwrap();
        fs::create_dir_all(custom.join("versions/26.3")).unwrap();
        fs::write(custom.join("versions/26.3/26.3.jar"), b"jar").unwrap();
        let mut detection = detect_at(
            Some(&official),
            "26.3",
            "fabric-loader-0.20.1-26.3",
            &[],
            &[official.clone()],
        );
        assert!(!detection.minecraft_version_installed);
        apply_chosen_vanilla(&mut detection, &[custom.clone()], true);
        assert!(detection.require_vanilla);
        assert!(detection.minecraft_version_installed);

        apply_chosen_vanilla(&mut detection, &[official.clone(), custom], true);
        assert!(!detection.minecraft_version_installed);
        assert_eq!(detection.game_dir.as_deref(), Some(official.as_path()));

        let mut optional = detect_at(Some(&official), "26.3", "fabric", &[], &[official.clone()]);
        apply_chosen_vanilla(&mut optional, &[], false);
        assert!(!optional.require_vanilla);
        assert!(!optional.minecraft_version_installed);
    }
}
