use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::manifest::{component_by_id, resolve_requires, ComponentKind, FileDest, Manifest};
use crate::quality::{row_for, table_from_manifest, QualityLevel};
use crate::state::InstalledState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub kit: String,
    #[serde(default)]
    pub custom_components: Vec<String>,
    #[serde(default)]
    pub ocean: bool,
    /// Optional add-ons other than Ocean. A missing key stays off. `defaultOn`
    /// is not applied here, so an older saved selection cannot gain SAO.
    #[serde(default)]
    pub addons: BTreeMap<String, bool>,
    pub snow_lite: Option<bool>,
    #[serde(default)]
    pub quality: Option<QualityLevel>,
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
    #[serde(default)]
    pub legacy_match: Vec<String>,
    #[serde(default)]
    pub known_sha512: Vec<String>,
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
    pub quality: QualityLevel,
    pub kit: String,
    pub components: Vec<String>,
    pub warnings: Vec<String>,
    pub bytes: u64,
}

fn push_addon_components(
    manifest: &Manifest,
    selected: &mut Vec<String>,
    skip: &[String],
    addon_id: &str,
) {
    let Some(addon) = manifest.addons.get(addon_id) else {
        return;
    };
    for id in &addon.components {
        if skip.iter().any(|skipped| skipped == id) {
            continue;
        }
        if let Some(component) = component_by_id(manifest, id) {
            if component.kind != ComponentKind::Pending && !selected.contains(id) {
                selected.push(id.clone());
            }
        }
    }
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

    let kit_lite = manifest
        .kits
        .get(&selection.kit)
        .map(|k| k.snow_lite)
        .unwrap_or(false);
    let hint_lite = selection.snow_lite.unwrap_or(kit_lite);
    let quality = selection.quality.unwrap_or(if hint_lite {
        QualityLevel::Lite
    } else {
        QualityLevel::Normal
    });
    let snow_lite = quality.snow_lite();
    let table = table_from_manifest(manifest);
    let skip = &row_for(&table, quality).skip_components;

    if selection.ocean {
        push_addon_components(manifest, &mut selected, skip, "ocean");
    }
    for (id, enabled) in &selection.addons {
        if !*enabled || id == "ocean" {
            continue;
        }
        push_addon_components(manifest, &mut selected, skip, id);
    }

    selected.retain(|id| {
        !skip.iter().any(|s| s == id)
            && component_by_id(manifest, id).is_some_and(|c| c.kind != ComponentKind::Pending)
    });

    let resolved = resolve_requires(manifest, &selected)?;

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
                legacy_match: component.legacy_match.clone(),
                known_sha512: component.known_sha512.clone(),
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
                    legacy_match: Vec::new(),
                    known_sha512: Vec::new(),
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
        quality,
        kit: kit_id.to_string(),
        components: resolved,
        warnings,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::load_fixture_manifest;
    use crate::state::{record_file, InstalledState};

    #[test]
    fn glob_matches_old_versions() {
        assert!(match_glob(
            "sodium-fabric-*.jar",
            "sodium-fabric-0.8.0+mc26.3.jar"
        ));
        assert!(match_glob("arnold-deck-*.jar", "arnold-deck-0.11.1.jar"));
        assert!(match_glob(
            "particlerain-*.jar",
            "particlerain-4.0.2+26.3-fabric.jar"
        ));
        assert!(!match_glob("particlerain-*.jar", "particle-rain-4.0.2.jar"));
        assert!(!match_glob("sodium-fabric-*.jar", "iris-1.11.7.jar"));
        assert!(!match_glob(
            "sodium-fabric-*.jar",
            "sodium-extra-fabric-0.9.5+mc26.3.jar"
        ));
        assert!(match_glob(
            "sodium-extra-*.jar",
            "sodium-extra-fabric-0.9.5+mc26.3.jar"
        ));
        assert!(!match_glob(
            "sodium-extra-*.jar",
            "sodium-fabric-0.9.2+mc26.3.jar"
        ));
    }

    #[test]
    fn custom_shaders_pulls_requires() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "custom".into(),
                custom_components: vec!["shaders".into()],
                ocean: false,
                addons: BTreeMap::new(),
                snow_lite: Some(false),
                quality: None,
            },
            None,
            true,
        )
        .unwrap();
        assert!(plan.components.contains(&"deck".into()));
        assert!(plan.components.contains(&"base".into()));
        assert!(plan.components.contains(&"sodium".into()));
        assert!(plan.components.contains(&"shaders".into()));
        assert!(
            !plan.warnings.iter().any(|w| w.contains("Deck")),
            "server Shipwright 0.11.2 meets Deck 0.11.1: {warnings:?}",
            warnings = plan.warnings
        );
    }

    #[test]
    fn pending_ocean_waves_never_planned() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: true,
                addons: BTreeMap::new(),
                snow_lite: None,
                quality: None,
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
        let manifest = load_fixture_manifest().unwrap();
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
                addons: BTreeMap::new(),
                snow_lite: Some(true),
                quality: None,
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

    #[test]
    fn potato_skips_heavy_ocean_even_when_ocean_is_on() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: true,
                addons: BTreeMap::new(),
                snow_lite: None,
                quality: Some(QualityLevel::Potato),
            },
            None,
            true,
        )
        .unwrap();
        assert_eq!(plan.quality, QualityLevel::Potato);
        assert!(plan.snow_lite);
        assert!(plan.components.contains(&"particlerain".into()));
        assert!(!plan.components.contains(&"wakes".into()));
        assert!(!plan.components.contains(&"waves".into()));
        assert!(!plan.components.contains(&"saoaincrad".into()));
        assert!(!plan.components.contains(&"geckolib".into()));
    }

    #[test]
    fn quality_drives_snow_lite() {
        let manifest = load_fixture_manifest().unwrap();
        let ultra = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                addons: BTreeMap::new(),
                snow_lite: Some(true),
                quality: Some(QualityLevel::Ultra),
            },
            None,
            true,
        )
        .unwrap();
        assert!(!ultra.snow_lite);
        assert_eq!(ultra.quality, QualityLevel::Ultra);
        assert!(!ultra.components.contains(&"saoaincrad".into()));
    }

    fn selection_from(raw: &str) -> Selection {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn saved_selection_without_sao_stays_off_even_if_default_on_is_flipped() {
        let mut manifest = load_fixture_manifest().unwrap();
        let sao = manifest.addons.get_mut("sao").unwrap();
        for kit in ["basic", "full", "recommended"] {
            assert_eq!(sao.default_on.get(kit).copied(), Some(false));
            sao.default_on.insert(kit.into(), true);
        }
        for (kit_id, kit) in &manifest.kits {
            assert!(
                !kit.components
                    .iter()
                    .any(|id| id == "saoaincrad" || id == "geckolib"),
                "{kit_id}"
            );
        }
        let table = manifest.quality_presets.clone().unwrap();
        for row in [&table.potato, &table.lite, &table.normal, &table.ultra] {
            assert!(!row
                .skip_components
                .iter()
                .any(|id| id == "saoaincrad" || id == "geckolib"));
        }
        let saved =
            selection_from(r#"{"kit":"full","ocean":true,"snowLite":false,"quality":"ultra"}"#);
        assert!(saved.addons.is_empty());
        let plan = build_plan(&manifest, &saved, None, true).unwrap();
        assert!(plan.components.contains(&"wakes".into()));
        assert!(!plan.components.contains(&"saoaincrad".into()));
        assert!(!plan.components.contains(&"geckolib".into()));
    }

    #[test]
    fn kits_and_quality_do_not_turn_sao_on() {
        let manifest = load_fixture_manifest().unwrap();
        for kit in ["basic", "full", "recommended"] {
            for quality in [
                QualityLevel::Potato,
                QualityLevel::Lite,
                QualityLevel::Normal,
                QualityLevel::Ultra,
            ] {
                let plan = build_plan(
                    &manifest,
                    &Selection {
                        kit: kit.into(),
                        custom_components: vec![],
                        ocean: true,
                        addons: BTreeMap::new(),
                        snow_lite: None,
                        quality: Some(quality),
                    },
                    None,
                    true,
                )
                .unwrap();
                assert!(
                    !plan
                        .components
                        .iter()
                        .any(|id| id == "saoaincrad" || id == "geckolib"),
                    "{kit} {quality:?} {components:?}",
                    components = plan.components
                );
            }
        }
    }

    #[test]
    fn sao_opt_in_installs_aincrad_geckolib_and_fabric() {
        let manifest = load_fixture_manifest().unwrap();
        let mut addons = BTreeMap::new();
        addons.insert("sao".into(), false);
        let off = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                addons: addons.clone(),
                snow_lite: None,
                quality: Some(QualityLevel::Normal),
            },
            None,
            true,
        )
        .unwrap();
        assert!(!off.components.contains(&"saoaincrad".into()));

        addons.insert("sao".into(), true);
        addons.insert("ocean".into(), true);
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                addons,
                snow_lite: None,
                quality: Some(QualityLevel::Normal),
            },
            None,
            true,
        )
        .unwrap();
        assert!(plan.components.contains(&"saoaincrad".into()));
        assert!(plan.components.contains(&"geckolib".into()));
        assert!(plan.components.contains(&"base".into()));
        assert!(!plan.components.contains(&"particlerain".into()));
        let sao_at = plan
            .components
            .iter()
            .position(|id| id == "saoaincrad")
            .unwrap();
        let gecko_at = plan
            .components
            .iter()
            .position(|id| id == "geckolib")
            .unwrap();
        let base_at = plan.components.iter().position(|id| id == "base").unwrap();
        assert!(base_at < gecko_at && gecko_at < sao_at);
    }
}
