use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detect::HostOs;
use crate::profiles::TLAUNCHER_PROFILES_FILE;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub fn launcher_note(pack_name: &str, launcher_label: &str) -> String {
    format!("Open {launcher_label} and choose the 'Arnold — {pack_name}' profile.")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherKind {
    Official,
    TLauncher,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherPrefs {
    #[serde(default)]
    pub write_official: Option<bool>,
    #[serde(default)]
    pub write_tlauncher: Option<bool>,
    #[serde(default)]
    pub play: Option<LauncherKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TLauncherFound {
    pub open_target: Option<PathBuf>,
    pub game_dir: PathBuf,
    /// Every TLauncher path below is unverified on a real machine.
    pub verified: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherReport {
    pub official: Option<FoundLauncher>,
    pub official_root: PathBuf,
    pub tlauncher: Option<TLauncherFound>,
    pub play_default: Option<LauncherKind>,
}

#[derive(Debug, Clone)]
pub struct TLauncherCandidates {
    pub config_dir: PathBuf,
    pub properties: PathBuf,
    pub exe_dir: PathBuf,
    pub start_menu_dir: PathBuf,
    pub desktop_dir: PathBuf,
    pub app_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TLauncherSight {
    pub config_dir_exists: bool,
    pub open_target: Option<PathBuf>,
    pub profiles_exists: bool,
    pub properties_text: Option<String>,
    pub game_dir_fallback: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LauncherWorld {
    pub os: HostOs,
    pub official: LauncherProbe,
    pub official_root: PathBuf,
    pub tlauncher: Option<TLauncherSight>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDestinations {
    pub vanilla_roots: Vec<PathBuf>,
    pub official: Option<PathBuf>,
    pub tlauncher: Option<PathBuf>,
    pub also_pack_dir: bool,
    pub require_vanilla: bool,
}

pub fn availability_from(found: Option<&FoundLauncher>) -> LauncherAvailability {
    LauncherAvailability {
        available: found.is_some(),
        note: launcher_note("Better Adventures+", "Minecraft Launcher"),
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
    let report = detect_launchers();
    let Some(kind) = choose_play_target(&report, &LauncherPrefs::default()) else {
        return LauncherOpenResult {
            opened: false,
            note: launcher_note("Better Adventures+", "Minecraft Launcher"),
        };
    };
    open(kind, &report, "Better Adventures+")
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

/// Candidate key `minecraft.gamedir` is VERIFY on a real machine.
pub fn tlauncher_game_dir_from_properties(text: &str) -> Option<PathBuf> {
    let unfolded = unfold_property_lines(text);
    for line in unfolded.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        let Some((key, value)) = split_property(trimmed) else {
            continue;
        };
        if key == "minecraft.gamedir" {
            let decoded = unescape_properties(value.trim());
            if decoded.is_empty() {
                return None;
            }
            return Some(PathBuf::from(decoded));
        }
    }
    None
}

fn unfold_property_lines(text: &str) -> String {
    let mut out = String::new();
    let mut continued = false;
    for line in text.lines() {
        let (body, continues) = strip_continuation(line);
        if continued {
            out.push_str(body.trim_start());
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(body);
        }
        continued = continues;
    }
    out
}

fn strip_continuation(line: &str) -> (&str, bool) {
    let mut count = 0;
    for ch in line.chars().rev() {
        if ch == '\\' {
            count += 1;
        } else {
            break;
        }
    }
    if count % 2 == 1 {
        (&line[..line.len() - 1], true)
    } else {
        (line, false)
    }
}

fn split_property(line: &str) -> Option<(String, &str)> {
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '=' || ch == ':' {
            let key = unescape_properties(line[..index].trim());
            return Some((key, &line[index + ch.len_utf8()..]));
        }
    }
    None
}

fn unescape_properties(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                    if let Some(decoded) = char::from_u32(code) {
                        out.push(decoded);
                    }
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// TLauncher is probed on Windows and macOS only.
/// VERIFY on a real Windows machine: `%AppData%\.tlauncher\`, `tlauncher-2.0.properties`,
/// `TLauncher*.exe` in that folder, Start Menu `Programs\TLauncher\TLauncher*.lnk`,
/// and a Desktop `TLauncher*.lnk`.
/// VERIFY on a real Mac: `~/Library/Application Support/tlauncher/`,
/// `/Applications/TLauncher.app`, and `~/Applications/TLauncher.app`.
pub fn tlauncher_candidates(
    home: &Path,
    appdata: Option<&Path>,
    os: HostOs,
) -> TLauncherCandidates {
    match os {
        HostOs::Windows => {
            let roaming = appdata
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join("AppData").join("Roaming"));
            let config = roaming.join(".tlauncher");
            TLauncherCandidates {
                config_dir: config.clone(),
                properties: config.join("tlauncher-2.0.properties"),
                exe_dir: config,
                start_menu_dir: roaming
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs")
                    .join("TLauncher"),
                desktop_dir: home.join("Desktop"),
                app_paths: Vec::new(),
            }
        }
        HostOs::Macos => {
            let config = home
                .join("Library")
                .join("Application Support")
                .join("tlauncher");
            TLauncherCandidates {
                properties: config.join("tlauncher-2.0.properties"),
                config_dir: config,
                exe_dir: PathBuf::new(),
                start_menu_dir: PathBuf::new(),
                desktop_dir: home.join("Desktop"),
                app_paths: vec![
                    PathBuf::from("/Applications/TLauncher.app"),
                    home.join("Applications").join("TLauncher.app"),
                ],
            }
        }
        HostOs::Linux => TLauncherCandidates {
            config_dir: PathBuf::new(),
            properties: PathBuf::new(),
            exe_dir: PathBuf::new(),
            start_menu_dir: PathBuf::new(),
            desktop_dir: PathBuf::new(),
            app_paths: Vec::new(),
        },
    }
}

pub fn report_from_world(world: &LauncherWorld) -> LauncherReport {
    let official = found_from_probe(&world.official);
    let tlauncher = if world.os == HostOs::Linux {
        None
    } else {
        world.tlauncher.as_ref().and_then(tlauncher_from_sight)
    };
    let play_default = match (&official, &tlauncher) {
        (Some(_), _) => Some(LauncherKind::Official),
        (None, Some(found)) if found.open_target.is_some() => Some(LauncherKind::TLauncher),
        _ => None,
    };
    LauncherReport {
        official,
        official_root: world.official_root.clone(),
        tlauncher,
        play_default,
    }
}

fn tlauncher_from_sight(sight: &TLauncherSight) -> Option<TLauncherFound> {
    let found = sight.config_dir_exists || sight.open_target.is_some() || sight.profiles_exists;
    if !found {
        return None;
    }
    let game_dir = sight
        .properties_text
        .as_deref()
        .and_then(tlauncher_game_dir_from_properties)
        .unwrap_or_else(|| sight.game_dir_fallback.clone());
    Some(TLauncherFound {
        open_target: sight.open_target.clone(),
        game_dir,
        verified: false,
    })
}

pub fn choose_play_target(report: &LauncherReport, prefs: &LauncherPrefs) -> Option<LauncherKind> {
    if let Some(kind) = prefs.play {
        if kind_openable(kind, report) {
            return Some(kind);
        }
    }
    report
        .play_default
        .filter(|kind| kind_openable(*kind, report))
}

fn kind_openable(kind: LauncherKind, report: &LauncherReport) -> bool {
    match kind {
        LauncherKind::Official => report.official.is_some(),
        LauncherKind::TLauncher => report
            .tlauncher
            .as_ref()
            .and_then(|found| found.open_target.as_ref())
            .is_some(),
    }
}

pub fn profile_destinations(
    report: &LauncherReport,
    prefs: &LauncherPrefs,
    pack_dir: &Path,
) -> ProfileDestinations {
    let write_official = prefs.write_official.unwrap_or(true);
    let write_tl = prefs.write_tlauncher.unwrap_or(true);
    let official_found = report.official.is_some();
    if official_found {
        if let Some(tl) = &report.tlauncher {
            let mut roots = vec![report.official_root.clone()];
            if tl.game_dir != report.official_root {
                roots.push(tl.game_dir.clone());
            }
            return ProfileDestinations {
                vanilla_roots: roots,
                official: write_official.then(|| report.official_root.clone()),
                tlauncher: write_tl.then(|| tl.game_dir.clone()),
                also_pack_dir: false,
                require_vanilla: true,
            };
        }
        return ProfileDestinations {
            vanilla_roots: vec![report.official_root.clone()],
            official: write_official.then(|| report.official_root.clone()),
            tlauncher: None,
            also_pack_dir: false,
            require_vanilla: true,
        };
    }
    if let Some(tl) = &report.tlauncher {
        let same_folder = tl.game_dir == report.official_root;
        let official = if same_folder
            && report
                .official_root
                .join("launcher_profiles.json")
                .is_file()
            && write_official
        {
            Some(report.official_root.clone())
        } else {
            None
        };
        return ProfileDestinations {
            vanilla_roots: vec![tl.game_dir.clone()],
            official,
            tlauncher: write_tl.then(|| tl.game_dir.clone()),
            also_pack_dir: false,
            require_vanilla: true,
        };
    }
    ProfileDestinations {
        vanilla_roots: Vec::new(),
        official: Some(report.official_root.clone()),
        tlauncher: None,
        also_pack_dir: pack_dir != report.official_root,
        require_vanilla: false,
    }
}

pub fn tlauncher_launch_plan(target: &Path) -> LaunchPlan {
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".lnk") {
        return LaunchPlan {
            program: "explorer.exe".into(),
            args: vec![target.display().to_string()],
            label: "TLauncher".into(),
        };
    }
    if name.ends_with(".app") {
        return LaunchPlan {
            program: "open".into(),
            args: vec!["-a".into(), target.display().to_string()],
            label: "TLauncher".into(),
        };
    }
    LaunchPlan {
        program: target.display().to_string(),
        args: Vec::new(),
        label: "TLauncher".into(),
    }
}

pub fn open_plan(kind: LauncherKind, report: &LauncherReport) -> Option<LaunchPlan> {
    match kind {
        LauncherKind::Official => report.official.as_ref().map(launch_plan),
        LauncherKind::TLauncher => report
            .tlauncher
            .as_ref()
            .and_then(|found| found.open_target.as_ref())
            .map(|path| tlauncher_launch_plan(path)),
    }
}

pub fn open(kind: LauncherKind, report: &LauncherReport, pack_name: &str) -> LauncherOpenResult {
    let label = match kind {
        LauncherKind::Official => "Minecraft Launcher",
        LauncherKind::TLauncher => "TLauncher",
    };
    let note = launcher_note(pack_name, label);
    let Some(plan) = open_plan(kind, report) else {
        return LauncherOpenResult {
            opened: false,
            note,
        };
    };
    let opened =
        crate::process::spawn_detached(crate::process::command(&plan.program).args(&plan.args))
            .is_ok();
    LauncherOpenResult { opened, note }
}

pub fn detect_launchers() -> LauncherReport {
    let os = crate::detect::current_os();
    let official_root = crate::detect::default_candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("."));
    let tlauncher = if os == HostOs::Linux {
        None
    } else {
        Some(live_tlauncher_sight(os, &official_root))
    };
    report_from_world(&LauncherWorld {
        os,
        official: live_probe(),
        official_root,
        tlauncher,
    })
}

fn live_tlauncher_sight(os: HostOs, fallback: &Path) -> TLauncherSight {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(dirs::config_dir);
    let candidates = tlauncher_candidates(&home, appdata.as_deref(), os);
    let properties_text = fs::read_to_string(&candidates.properties).ok();
    let game_dir = properties_text
        .as_deref()
        .and_then(tlauncher_game_dir_from_properties)
        .unwrap_or_else(|| fallback.to_path_buf());
    TLauncherSight {
        config_dir_exists: candidates.config_dir.is_dir(),
        open_target: first_open_target(&candidates, os),
        profiles_exists: game_dir.join(TLAUNCHER_PROFILES_FILE).is_file()
            || fallback.join(TLAUNCHER_PROFILES_FILE).is_file(),
        properties_text,
        game_dir_fallback: fallback.to_path_buf(),
    }
}

fn first_open_target(candidates: &TLauncherCandidates, os: HostOs) -> Option<PathBuf> {
    match os {
        HostOs::Windows => first_named(&candidates.exe_dir, "TLauncher", ".exe")
            .or_else(|| first_named(&candidates.start_menu_dir, "TLauncher", ".lnk"))
            .or_else(|| first_named(&candidates.desktop_dir, "TLauncher", ".lnk")),
        HostOs::Macos => candidates
            .app_paths
            .iter()
            .find(|path| path.is_dir())
            .cloned(),
        HostOs::Linux => None,
    }
}

fn first_named(dir: &Path, prefix: &str, suffix: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(prefix) && name.to_ascii_lowercase().ends_with(suffix)
                })
        })
        .collect();
    found.sort();
    found.into_iter().next()
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
            launcher_note("Better Adventures+", "Minecraft Launcher"),
            "Open Minecraft Launcher and choose the 'Arnold — Better Adventures+' profile."
        );
        assert!(launcher_note("Better Adventures+", "Minecraft Launcher").contains('\u{2014}'));
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

    fn world(os: HostOs, official: bool, tl: bool, openable: bool) -> LauncherWorld {
        let root = PathBuf::from("/games/minecraft");
        LauncherWorld {
            os,
            official: LauncherProbe {
                os,
                store_installed: official && os == HostOs::Windows,
                classic_exes: Vec::new(),
                mac_app: official && os == HostOs::Macos,
                linux_native: official && os == HostOs::Linux,
                linux_flatpak: false,
            },
            official_root: root.clone(),
            tlauncher: Some(TLauncherSight {
                config_dir_exists: tl,
                open_target: openable.then(|| PathBuf::from("/apps/TLauncher.exe")),
                profiles_exists: false,
                properties_text: None,
                game_dir_fallback: root,
            }),
        }
    }

    #[test]
    fn probe_matrix_picks_play_and_write_targets() {
        for os in [HostOs::Windows, HostOs::Macos] {
            let both = report_from_world(&world(os, true, true, true));
            assert!(both.official.is_some());
            assert!(both.tlauncher.is_some());
            assert_eq!(both.play_default, Some(LauncherKind::Official));
            let dest =
                profile_destinations(&both, &LauncherPrefs::default(), Path::new("/packs/ba"));
            assert!(dest.official.is_some());
            assert!(dest.tlauncher.is_some());
            assert!(dest.require_vanilla);

            let official_only = report_from_world(&world(os, true, false, false));
            assert!(official_only.tlauncher.is_none());
            assert_eq!(official_only.play_default, Some(LauncherKind::Official));

            let tl_only = report_from_world(&world(os, false, true, true));
            assert!(tl_only.official.is_none());
            assert_eq!(tl_only.play_default, Some(LauncherKind::TLauncher));

            let neither = report_from_world(&world(os, false, false, false));
            assert!(neither.official.is_none());
            assert!(neither.tlauncher.is_none());
            assert!(neither.play_default.is_none());
            let dest =
                profile_destinations(&neither, &LauncherPrefs::default(), Path::new("/packs/ba"));
            assert!(!dest.require_vanilla);
            assert!(dest.official.is_some());
            assert!(dest.also_pack_dir);
        }

        let mut prefs = LauncherPrefs::default();
        prefs.play = Some(LauncherKind::TLauncher);
        let both = report_from_world(&world(HostOs::Windows, true, true, true));
        assert_eq!(
            choose_play_target(&both, &prefs),
            Some(LauncherKind::TLauncher)
        );

        let linux = report_from_world(&world(HostOs::Linux, true, true, true));
        assert!(linux.tlauncher.is_none());
        assert_eq!(linux.play_default, Some(LauncherKind::Official));
        let linux_only_tl = report_from_world(&world(HostOs::Linux, false, true, true));
        assert!(linux_only_tl.tlauncher.is_none());
        assert!(linux_only_tl.play_default.is_none());
    }

    #[test]
    fn properties_parser_handles_comments_and_escaped_windows_paths() {
        let text = "# comment\n! also\nminecraft.gamedir=C:\\\\Users\\\\Sam\\\\.minecraft\n";
        let path = tlauncher_game_dir_from_properties(text).unwrap();
        assert_eq!(path, PathBuf::from(r"C:\Users\Sam\.minecraft"));
        assert!(tlauncher_game_dir_from_properties("# nothing\nfoo=bar\n").is_none());
        let present = "minecraft.gamedir=/home/sam/.minecraft\n";
        assert_eq!(
            tlauncher_game_dir_from_properties(present).unwrap(),
            PathBuf::from("/home/sam/.minecraft")
        );
    }

    #[test]
    fn shortcut_and_app_open_plans() {
        let lnk = tlauncher_launch_plan(Path::new(r"C:\Users\Sam\Desktop\TLauncher.lnk"));
        assert_eq!(lnk.program, "explorer.exe");
        assert_eq!(lnk.args, vec![r"C:\Users\Sam\Desktop\TLauncher.lnk"]);
        let app = tlauncher_launch_plan(Path::new("/Applications/TLauncher.app"));
        assert_eq!(app.program, "open");
        assert_eq!(
            app.args,
            vec!["-a".to_string(), "/Applications/TLauncher.app".to_string()]
        );
    }

    #[test]
    fn sources_do_not_link_to_tlauncher_or_a_minecraft_uri() {
        for source in [
            include_str!("launcher.rs"),
            include_str!("profiles.rs"),
            include_str!("commands.rs"),
        ] {
            let lower = source.to_ascii_lowercase();
            assert!(!lower.contains("tlauncher.org"));
            assert!(!lower.contains("minecraft:"));
            let mut start = 0;
            while let Some(found) = lower[start..].find("tlauncher") {
                let index = start + found;
                let from = index.saturating_sub(80);
                let to = (index + "tlauncher".len() + 80).min(lower.len());
                assert!(
                    !lower[from..to].contains("http"),
                    "http near tlauncher: {}",
                    &lower[from..to]
                );
                start = index + "tlauncher".len();
            }
        }
    }
}
