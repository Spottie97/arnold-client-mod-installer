use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::detect::HostOs;
use crate::fabric::ARNOLD_PROFILE_NAME;

/// Minecraft Launcher from the Microsoft Store.
pub const STORE_PACKAGE_FAMILY: &str = "Microsoft.4297127D64EC6_8wekyb3d8bbwe";

pub const STORE_LAUNCH_ARG: &str =
    "shell:AppsFolder\\Microsoft.4297127D64EC6_8wekyb3d8bbwe!Minecraft";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundLauncher {
    WindowsStore,
    WindowsClassic(PathBuf),
    MacApp,
    LinuxNative,
    LinuxFlatpak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherProbe {
    pub os: HostOs,
    pub store_installed: bool,
    pub classic_exes: Vec<PathBuf>,
    pub mac_app: bool,
    pub linux_native: bool,
    pub linux_flatpak: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherAvailability {
    pub available: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherOpenResult {
    pub opened: bool,
    pub note: String,
}

pub fn launcher_note() -> String {
    format!("Open Minecraft and choose the '{ARNOLD_PROFILE_NAME}' profile.")
}

pub fn availability_from(found: Option<&FoundLauncher>) -> LauncherAvailability {
    LauncherAvailability {
        available: found.is_some(),
        note: launcher_note(),
    }
}

/// Store launcher first, then `MinecraftLauncher.exe` under Program Files (x86) and Program Files.
/// Nothing else. The `minecraft:` protocol is not a launcher we can trust.
pub fn found_from_probe(probe: &LauncherProbe) -> Option<FoundLauncher> {
    match probe.os {
        HostOs::Windows => {
            if probe.store_installed {
                return Some(FoundLauncher::WindowsStore);
            }
            probe
                .classic_exes
                .iter()
                .find(|path| !path.as_os_str().is_empty())
                .map(|path| FoundLauncher::WindowsClassic(path.clone()))
        }
        HostOs::Macos => probe.mac_app.then_some(FoundLauncher::MacApp),
        HostOs::Linux => {
            if probe.linux_native {
                Some(FoundLauncher::LinuxNative)
            } else if probe.linux_flatpak {
                Some(FoundLauncher::LinuxFlatpak)
            } else {
                None
            }
        }
    }
}

pub fn classic_minecraft_launcher_paths(
    program_files: Option<&Path>,
    program_files_x86: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(root) = program_files_x86 {
        paths.push(
            root.join("Minecraft Launcher")
                .join("MinecraftLauncher.exe"),
        );
    }
    if let Some(root) = program_files {
        paths.push(
            root.join("Minecraft Launcher")
                .join("MinecraftLauncher.exe"),
        );
    }
    paths
}

pub fn launch_plan(found: &FoundLauncher) -> LaunchPlan {
    match found {
        FoundLauncher::WindowsStore => LaunchPlan {
            program: "explorer.exe".into(),
            args: vec![STORE_LAUNCH_ARG.into()],
            label: "Microsoft Store".into(),
        },
        FoundLauncher::WindowsClassic(path) => LaunchPlan {
            program: path.display().to_string(),
            args: Vec::new(),
            label: "Minecraft Launcher".into(),
        },
        FoundLauncher::MacApp => LaunchPlan {
            program: "open".into(),
            args: vec!["-a".into(), "Minecraft".into()],
            label: "Minecraft.app".into(),
        },
        FoundLauncher::LinuxNative => LaunchPlan {
            program: "minecraft-launcher".into(),
            args: Vec::new(),
            label: "minecraft-launcher".into(),
        },
        FoundLauncher::LinuxFlatpak => LaunchPlan {
            program: "flatpak".into(),
            args: vec!["run".into(), "com.mojang.Minecraft".into()],
            label: "Flatpak Minecraft".into(),
        },
    }
}

pub fn appx_lists_package(output: &str, family: &str) -> bool {
    output
        .to_ascii_lowercase()
        .contains(&family.to_ascii_lowercase())
}

pub fn availability() -> LauncherAvailability {
    availability_from(detect().as_ref())
}

/// The installer exits only after the launcher process was actually spawned.
/// A miss or a failed spawn stays on the screen so the profile note can be shown.
pub fn should_exit_after_launch(opened: bool) -> bool {
    opened
}

pub fn try_open() -> LauncherOpenResult {
    let note = launcher_note();
    let Some(found) = detect() else {
        return LauncherOpenResult {
            opened: false,
            note,
        };
    };
    let plan = launch_plan(&found);
    let opened =
        crate::process::spawn_detached(crate::process::command(&plan.program).args(&plan.args))
            .is_ok();
    LauncherOpenResult { opened, note }
}

fn detect() -> Option<FoundLauncher> {
    found_from_probe(&live_probe())
}

fn live_probe() -> LauncherProbe {
    match crate::detect::current_os() {
        HostOs::Windows => LauncherProbe {
            os: HostOs::Windows,
            store_installed: windows_store_installed(),
            classic_exes: existing_classic_launchers(),
            mac_app: false,
            linux_native: false,
            linux_flatpak: false,
        },
        HostOs::Macos => LauncherProbe {
            os: HostOs::Macos,
            store_installed: false,
            classic_exes: Vec::new(),
            mac_app: mac_minecraft_app(),
            linux_native: false,
            linux_flatpak: false,
        },
        HostOs::Linux => {
            let (linux_native, linux_flatpak) = linux_launchers();
            LauncherProbe {
                os: HostOs::Linux,
                store_installed: false,
                classic_exes: Vec::new(),
                mac_app: false,
                linux_native,
                linux_flatpak,
            }
        }
    }
}

fn windows_store_installed() -> bool {
    if store_package_dir_exists() {
        return true;
    }
    store_appx_listed()
}

fn store_package_dir_exists() -> bool {
    let Some(local) = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir)
    else {
        return false;
    };
    local.join("Packages").join(STORE_PACKAGE_FAMILY).is_dir()
}

fn store_appx_listed() -> bool {
    let script = format!(
        "Get-AppxPackage | Where-Object {{ $_.PackageFamilyName -eq '{STORE_PACKAGE_FAMILY}' }} | Select-Object -ExpandProperty PackageFamilyName"
    );
    let Ok(output) = crate::process::command("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
    else {
        return false;
    };
    output.status.success()
        && appx_lists_package(
            &String::from_utf8_lossy(&output.stdout),
            STORE_PACKAGE_FAMILY,
        )
}

fn existing_classic_launchers() -> Vec<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
    classic_minecraft_launcher_paths(program_files.as_deref(), program_files_x86.as_deref())
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
}

fn mac_minecraft_app() -> bool {
    Path::new("/Applications/Minecraft.app").is_dir()
        || dirs::home_dir()
            .is_some_and(|home| home.join("Applications").join("Minecraft.app").is_dir())
}

fn linux_launchers() -> (bool, bool) {
    let native = program_on_path("minecraft-launcher");
    let flatpak = Path::new("/var/lib/flatpak/app/com.mojang.Minecraft").is_dir()
        || dirs::home_dir().is_some_and(|home| {
            home.join(".local/share/flatpak/app/com.mojang.Minecraft")
                .is_dir()
        });
    (native, flatpak)
}

fn program_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_launch_exits_and_a_failure_does_not() {
        assert!(should_exit_after_launch(true));
        assert!(!should_exit_after_launch(false));
    }

    #[test]
    fn exit_goes_through_app_exit_and_nothing_blocks_it() {
        let commands = include_str!("commands.rs");
        let lib = include_str!("lib.rs");
        assert!(commands.contains("app.exit(0)"));
        assert!(commands.contains("std::process::exit(0)"));
        for source in [commands, lib] {
            assert!(!source.contains("prevent_exit"));
            assert!(!source.contains("on_window_event"));
            assert!(!source.contains("ExitRequested"));
            assert!(!source.contains("thread::spawn"));
        }
    }

    #[test]
    fn note_uses_the_profile_the_installer_writes() {
        assert_eq!(
            launcher_note(),
            "Open Minecraft and choose the 'Arnold – Fabric 26.3' profile."
        );
        assert!(launcher_note().contains(ARNOLD_PROFILE_NAME));
    }

    #[test]
    fn windows_prefers_store_then_classic_and_never_the_protocol() {
        let classic = PathBuf::from(r"C:\Program Files\Minecraft Launcher\MinecraftLauncher.exe");
        let store = found_from_probe(&LauncherProbe {
            os: HostOs::Windows,
            store_installed: true,
            classic_exes: vec![classic.clone()],
            mac_app: false,
            linux_native: false,
            linux_flatpak: false,
        })
        .unwrap();
        assert_eq!(store, FoundLauncher::WindowsStore);
        let plan = launch_plan(&store);
        assert_eq!(plan.program, "explorer.exe");
        assert_eq!(plan.args, vec![STORE_LAUNCH_ARG]);
        assert!(!plan.args.iter().any(|arg| arg.contains("minecraft:")));

        let only_classic = found_from_probe(&LauncherProbe {
            os: HostOs::Windows,
            store_installed: false,
            classic_exes: vec![classic.clone()],
            mac_app: false,
            linux_native: false,
            linux_flatpak: false,
        })
        .unwrap();
        assert_eq!(only_classic, FoundLauncher::WindowsClassic(classic));

        assert!(found_from_probe(&LauncherProbe {
            os: HostOs::Windows,
            store_installed: false,
            classic_exes: Vec::new(),
            mac_app: false,
            linux_native: false,
            linux_flatpak: false,
        })
        .is_none());
    }

    #[test]
    fn classic_paths_cover_program_files_and_x86() {
        let paths = classic_minecraft_launcher_paths(
            Some(Path::new(r"C:\Program Files")),
            Some(Path::new(r"C:\Program Files (x86)")),
        );
        assert_eq!(paths.len(), 2);
        assert!(paths[0].to_string_lossy().contains("Program Files (x86)"));
        assert!(paths.iter().all(|path| path
            .file_name()
            .is_some_and(|name| name == "MinecraftLauncher.exe")));
    }

    #[test]
    fn appx_output_matches_the_store_family() {
        let sample = "Microsoft.4297127D64EC6_8wekyb3d8bbwe\r\n";
        assert!(appx_lists_package(sample, STORE_PACKAGE_FAMILY));
        assert!(!appx_lists_package("other\r\n", STORE_PACKAGE_FAMILY));
    }

    #[test]
    fn mac_and_linux_only_when_installed() {
        assert_eq!(
            found_from_probe(&LauncherProbe {
                os: HostOs::Macos,
                store_installed: false,
                classic_exes: Vec::new(),
                mac_app: true,
                linux_native: false,
                linux_flatpak: false,
            }),
            Some(FoundLauncher::MacApp)
        );
        assert!(found_from_probe(&LauncherProbe {
            os: HostOs::Macos,
            store_installed: false,
            classic_exes: Vec::new(),
            mac_app: false,
            linux_native: true,
            linux_flatpak: true,
        })
        .is_none());
        assert_eq!(
            found_from_probe(&LauncherProbe {
                os: HostOs::Linux,
                store_installed: false,
                classic_exes: Vec::new(),
                mac_app: false,
                linux_native: false,
                linux_flatpak: true,
            }),
            Some(FoundLauncher::LinuxFlatpak)
        );
        let plan = launch_plan(&FoundLauncher::LinuxFlatpak);
        assert_eq!(
            plan.args,
            vec!["run".to_string(), "com.mojang.Minecraft".to_string()]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("minecraft:")));
    }
}
