use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::manifest::{component_by_id, resolve_requires, ComponentKind, FileDest, Manifest};
use crate::state::InstalledState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub kit: String,
    #[serde(default)]
    pub custom_components: Vec<String>,
    #[serde(default)]
    pub ocean: bool,
    pub snow_lite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanFile {
    pub component_id: String,
    pub name: String,
    pub version: String,
    pub file: String,
    pub dest: FileDest,
    pub url: String,
    pub sha512: String,
    pub size: u64,
    pub match_glob: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanConfig {
    pub component_id: String,
    pub file: String,
    pub snow_lite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanLoader {
    pub id: String,
    pub url: String,
    pub sha512: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub add: Vec<PlanFile>,
    pub update: Vec<PlanFile>,
    pub remove: Vec<PlanFile>,
    pub configs: Vec<PlanConfig>,
    pub loader: Option<PlanLoader>,
    pub snow_lite: bool,
    pub kit: String,
    pub components: Vec<String>,
    pub warnings: Vec<String>,
    pub bytes: u64,
}

pub fn match_glob(pattern: &str, name: &str) -> bool {
    if pattern == name {
        return true;
    }
    let Some((head, tail)) = pattern.split_once('*') else {
        return false;
    };
    if tail.contains('*') {
        return name.starts_with(head)
            && name.ends_with(tail.rsplit_once('*').map(|(_, t)| t).unwrap_or(tail));
    }
    name.starts_with(head) && name.ends_with(tail)
}

fn version_parts(raw: &str) -> Vec<u32> {
    raw.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

pub fn version_less(a: &str, b: &str) -> bool {
    let left = version_parts(a);
    let right = version_parts(b);
    let len = left.len().max(right.len());
    for i in 0..len {
        let l = left.get(i).copied().unwrap_or(0);
        let r = right.get(i).copied().unwrap_or(0);
        if l != r {
            return l < r;
        }
    }
    false
}

pub fn build_plan(
    manifest: &Manifest,
    selection: &Selection,
    state: Option<&InstalledState>,
    fabric_installed: bool,
) -> Result<InstallPlan, AppError> {
    let kit_id = if selection.kit == "custom" {
        "custom"
    } else if selection.kit == "recommended" {
        "recommended"
    } else {
        selection.kit.as_str()
    };

    let mut selected = if selection.kit == "custom" {
        selection.custom_components.clone()
    } else {
        let kit = manifest
            .kits
            .get(&selection.kit)
            .ok_or_else(|| AppError::ManifestSchema(format!("unknown kit {}", selection.kit)))?;
        kit.components.clone()
    };

    for component in &manifest.components {
        if component.required_in_all_kits && !selected.iter().any(|id| id == &component.id) {
            selected.push(component.id.clone());
        }
    }

    if selection.ocean {
        if let Some(ocean) = manifest.addons.get("ocean") {
            for id in &ocean.components {
                if let Some(c) = component_by_id(manifest, id) {
                    if c.kind != ComponentKind::Pending && !selected.contains(id) {
                        selected.push(id.clone());
                    }
                }
            }
        }
    }

    selected.retain(|id| {
        component_by_id(manifest, id).is_some_and(|c| c.kind != ComponentKind::Pending)
    });

    let resolved = resolve_requires(manifest, &selected)?;
    let snow_lite = selection.snow_lite.unwrap_or_else(|| {
        manifest
            .kits
            .get(&selection.kit)
            .map(|k| k.snow_lite)
            .unwrap_or(false)
    });

    let mut add = Vec::new();
    let mut update = Vec::new();
    let mut configs = Vec::new();
    let mut warnings = Vec::new();
    let mut bytes = 0u64;

    for id in &resolved {
        let component = component_by_id(manifest, id).expect("resolved id");
        if let (Some(need), Some(have)) = (
            component.min_server_shipwright.as_deref(),
            manifest.current_server_shipwright.as_deref(),
        ) {
            if version_less(have, need) {
                warnings.push(format!(
                    "{} {} needs server Shipwright {}; the server is on {} until its next update. It will still install, but it may misbehave until then.",
                    component.name, component.version, need, have
                ));
            }
        }
        if component.config.is_some() {
            configs.push(PlanConfig {
                component_id: component.id.clone(),
                file: component.config.as_ref().unwrap().file.clone(),
                snow_lite: component.id == "snow" && snow_lite,
            });
        }
        for file in &component.files {
            let item = PlanFile {
                component_id: component.id.clone(),
                name: component.name.clone(),
                version: component.version.clone(),
                file: file.file.clone(),
                dest: file.dest,
                url: file.url.clone(),
                sha512: file.sha512.clone(),
                size: file.size,
                match_glob: file.match_glob.clone(),
            };
            bytes += file.size;
            let dest_rel = format!(
                "{}/{}",
                match file.dest {
                    FileDest::Mods => "mods",
                    FileDest::Shaderpacks => "shaderpacks",
                },
                file.file
            );
            match state.and_then(|s| s.file_by_path(&dest_rel)) {
                Some(existing) if existing.sha512 == file.sha512 => {}
                Some(_) => update.push(item),
                None => add.push(item),
            }
        }
    }

    let mut remove = Vec::new();
    if let Some(state) = state {
        for tracked in &state.files {
            if !resolved.iter().any(|id| id == &tracked.component_id) {
                remove.push(PlanFile {
                    component_id: tracked.component_id.clone(),
                    name: tracked.component_id.clone(),
                    version: String::new(),
                    file: tracked.path.clone(),
                    dest: FileDest::Mods,
                    url: String::new(),
                    sha512: tracked.sha512.clone(),
                    size: 0,
                    match_glob: String::new(),
                });
            }
        }
    }

    let loader = if fabric_installed {
        None
    } else {
        Some(PlanLoader {
            id: manifest.loader.id.clone(),
            url: manifest.loader.url.clone(),
            sha512: manifest.loader.sha512.clone(),
        })
    };

    Ok(InstallPlan {
        add,
        update,
        remove,
        configs,
        loader,
        snow_lite,
        kit: kit_id.to_string(),
        components: resolved,
        warnings,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::load_bundled_manifest;
    use crate::state::{record_file, InstalledState};

    #[test]
    fn glob_matches_old_versions() {
        assert!(match_glob(
            "sodium-fabric-*.jar",
            "sodium-fabric-0.8.0+mc26.3.jar"
        ));
        assert!(match_glob("arnold-deck-*.jar", "arnold-deck-0.11.1.jar"));
        assert!(!match_glob("sodium-fabric-*.jar", "iris-1.11.7.jar"));
    }

    #[test]
    fn custom_shaders_pulls_requires() {
        let manifest = load_bundled_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "custom".into(),
                custom_components: vec!["shaders".into()],
                ocean: false,
                snow_lite: Some(false),
            },
            None,
            true,
        )
        .unwrap();
        assert!(plan.components.contains(&"deck".into()));
        assert!(plan.components.contains(&"base".into()));
        assert!(plan.components.contains(&"sodium".into()));
        assert!(plan.components.contains(&"shaders".into()));
        assert!(plan.warnings.iter().any(|w| w.contains("Deck")));
    }

    #[test]
    fn pending_ocean_waves_never_planned() {
        let manifest = load_bundled_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: true,
                snow_lite: None,
            },
            None,
            false,
        )
        .unwrap();
        assert!(!plan.components.contains(&"waves".into()));
        assert!(plan.components.contains(&"particlerain".into()));
        assert!(plan.loader.is_some());
        assert!(plan.snow_lite);
    }

    #[test]
    fn plan_diff_update_and_remove() {
        let manifest = load_bundled_manifest().unwrap();
        let mut state = InstalledState::new("installer", "old");
        record_file(
            &mut state,
            "mods/sodium-fabric-0.9.2+mc26.3.jar",
            "oldhash",
            "sodium",
        );
        record_file(&mut state, "mods/gone.jar", "x", "removedmod");
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                snow_lite: Some(true),
            },
            Some(&state),
            true,
        )
        .unwrap();
        assert!(plan.update.iter().any(|f| f.component_id == "sodium"));
        assert!(plan.remove.iter().any(|f| f.file == "mods/gone.jar"));
    }

    #[test]
    fn shipwright_compare() {
        assert!(version_less("0.11.0", "0.11.1"));
        assert!(!version_less("0.11.1", "0.11.0"));
        assert!(!version_less("0.11.1", "0.11.1"));
    }
}
