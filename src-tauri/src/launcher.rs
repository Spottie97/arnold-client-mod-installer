use std::path::{Path, PathBuf};
use std::process::Command;

use crate::detect::HostOs;
use crate::errors::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub label: String,
}

pub fn launcher_candidates(os: HostOs, program_files: Option<&Path>) -> Vec<LaunchPlan> {
    match os {
        HostOs::Windows => {
            let mut plans = Vec::new();
            if let Some(root) = program_files {
                plans.push(LaunchPlan {
                    program: root
                        .join("Minecraft Launcher")
                        .join("MinecraftLauncher.exe")
                        .to_string_lossy()
                        .into_owned(),
                    args: Vec::new(),
                    label: "Minecraft Launcher".into(),
                });
                plans.push(LaunchPlan {
                    program: root
                        .join("XboxGames")
                        .join("Minecraft Launcher")
                        .join("Content")
                        .join("Minecraft.exe")
                        .to_string_lossy()
                        .into_owned(),
                    args: Vec::new(),
                    label: "Xbox Minecraft Launcher".into(),
                });
            }
            plans.push(LaunchPlan {
                program: "cmd".into(),
                args: vec!["/C".into(), "start".into(), "".into(), "minecraft:".into()],
                label: "minecraft: protocol".into(),
            });
            plans
        }
        HostOs::Macos => vec![LaunchPlan {
            program: "open".into(),
            args: vec!["-a".into(), "Minecraft".into()],
            label: "Minecraft.app".into(),
        }],
        HostOs::Linux => vec![
            LaunchPlan {
                program: "minecraft-launcher".into(),
                args: Vec::new(),
                label: "minecraft-launcher".into(),
            },
            LaunchPlan {
                program: "flatpak".into(),
                args: vec!["run".into(), "com.mojang.Minecraft".into()],
                label: "Flatpak Minecraft".into(),
            },
            LaunchPlan {
                program: "xdg-open".into(),
                args: vec!["minecraft:".into()],
                label: "minecraft: protocol".into(),
            },
        ],
    }
}

pub fn open_minecraft_launcher() -> Result<String, AppError> {
    open_with(
        crate::detect::current_os(),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
    )
}

pub fn open_with(os: HostOs, program_files: Option<PathBuf>) -> Result<String, AppError> {
    let candidates = launcher_candidates(os, program_files.as_deref());
    let mut last = "no launcher candidate".to_string();
    for plan in candidates {
        match Command::new(&plan.program).args(&plan.args).spawn() {
            Ok(_) => return Ok(plan.label),
            Err(err) => last = format!("{}: {err}", plan.label),
        }
    }
    Err(AppError::Other {
        code: "E-LAUNCHER".into(),
        title: "The Minecraft Launcher could not be opened".into(),
        hint: "Open the Minecraft Launcher yourself and choose the Arnold – Fabric 26.3 profile."
            .into(),
        detail: last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_tries_classic_then_protocol() {
        let plans = launcher_candidates(HostOs::Windows, Some(Path::new(r"C:\Program Files")));
        assert!(plans[0].program.contains("MinecraftLauncher.exe"));
        assert_eq!(plans.last().unwrap().args.last().unwrap(), "minecraft:");
    }

    #[test]
    fn mac_uses_open_app() {
        let plans = launcher_candidates(HostOs::Macos, None);
        assert_eq!(plans[0].program, "open");
        assert_eq!(
            plans[0].args,
            vec!["-a".to_string(), "Minecraft".to_string()]
        );
    }

    #[test]
    fn linux_tries_native_then_flatpak() {
        let plans = launcher_candidates(HostOs::Linux, None);
        assert_eq!(plans[0].program, "minecraft-launcher");
        assert_eq!(
            plans[1].args,
            vec!["run".to_string(), "com.mojang.Minecraft".to_string()]
        );
    }
}
