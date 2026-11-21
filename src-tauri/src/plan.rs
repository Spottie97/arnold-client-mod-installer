use std::collections::BTreeMap;
use std::path::Path;

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
    /// is not applied here, so an older saved selection cannot gain a new addon.
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
    /// Components present only because an add-on toggle is on. Kit members,
    /// locked components, and direct custom picks are never listed here.
    #[serde(default)]
    pub optional_components: Vec<String>,
    pub warnings: Vec<String>,
    pub bytes: u64,
    /// Pack and folder this file list was built against. Install rejects any other target.
    #[serde(default)]
    pub pack_id: String,
    #[serde(default)]
    pub pack_dir: String,
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

pub fn diff_component_files(
    manifest: &Manifest,
    resolved: &[String],
    state: Option<&InstalledState>,
) -> (Vec<PlanFile>, Vec<PlanFile>, Vec<PlanFile>, u64) {
    let mut add = Vec::new();
    let mut update = Vec::new();
    let mut bytes = 0u64;
    for id in resolved {
        let component = component_by_id(manifest, id).expect("resolved id");
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
            let dest_rel = format!("{}/{}", file.dest.folder(), file.file);
            match state.and_then(|installed| installed.file_by_path(&dest_rel)) {
                Some(existing) if existing.sha512 == file.sha512 => {}
                Some(_) => update.push(item),
                None => add.push(item),
            }
        }
    }

    let mut remove = Vec::new();
    if let Some(installed) = state {
        for tracked in &installed.files {
            if !resolved.iter().any(|id| id == &tracked.component_id) {
                remove.push(PlanFile {
                    component_id: tracked.component_id.clone(),
                    name: tracked.component_id.clone(),
                    version: String::new(),
                    file: tracked.path.clone(),
                    dest: FileDest::from_rel_path(&tracked.path).unwrap_or(FileDest::Mods),
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
    (add, update, remove, bytes)
}

/// Reject a plan that was built for a different pack, a different folder, or a
/// different installed file set than the folder about to be written.
pub fn ensure_plan_current(
    plan: &InstallPlan,
    pack_id: &str,
    dest: &Path,
    manifest: &Manifest,
    installed: Option<&InstalledState>,
) -> Result<(), AppError> {
    if plan.pack_id.is_empty() || plan.pack_id != pack_id {
        return Err(AppError::State(format!(
            "install plan is for pack '{}', but '{pack_id}' is selected",
            plan.pack_id
        )));
    }
    let dest_key = dest.to_string_lossy();
    if plan.pack_dir.is_empty() || plan.pack_dir != dest_key {
        return Err(AppError::State(format!(
            "install plan is for '{}', but the selected folder is {dest_key}",
            plan.pack_dir
        )));
    }
    let (add, update, remove, _) = diff_component_files(manifest, &plan.components, installed);
    if plan.add != add || plan.update != update || plan.remove != remove {
        return Err(AppError::State(
            "install plan does not match the selected pack folder; open the pack again before installing"
                .into(),
        ));
    }
    Ok(())
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
    let mut base_selected = selected.clone();

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

    let keep = |id: &String| {
        !skip.iter().any(|s| s == id)
            && component_by_id(manifest, id).is_some_and(|c| c.kind != ComponentKind::Pending)
    };
    selected.retain(&keep);
    base_selected.retain(&keep);

    let base_resolved = resolve_requires(manifest, &base_selected)?;
    let resolved = resolve_requires(manifest, &selected)?;
    let base_ids: std::collections::BTreeSet<&str> =
        base_resolved.iter().map(String::as_str).collect();
    let optional_components: Vec<String> = resolved
        .iter()
        .filter(|id| !base_ids.contains(id.as_str()))
        .cloned()
        .collect();

    let mut configs = Vec::new();
    let mut warnings = Vec::new();

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
    }

    let (add, update, remove, bytes) = diff_component_files(manifest, &resolved, state);

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
        optional_components,
        warnings,
        bytes,
        pack_id: String::new(),
        pack_dir: String::new(),
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
        for id in [
            "dietplus",
            "glitchcore",
            "sereneseasons",
            "drinkbeer",
            "farmersdelight",
            "balm",
            "hardcorerevival",
        ] {
            assert!(
                plan.components.iter().any(|item| item == id),
                "custom must lock {id}"
            );
        }
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
        assert!(!plan.components.contains(&"architect".into()));
        assert!(!plan.components.contains(&"litematica".into()));
        assert!(!plan.components.contains(&"malilib".into()));
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
        assert!(!ultra.components.contains(&"architect".into()));
    }

    fn selection_from(raw: &str) -> Selection {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn saved_selection_without_architect_stays_off_even_if_default_on_is_flipped() {
        let mut manifest = load_fixture_manifest().unwrap();
        let architect = manifest.addons.get_mut("architect").unwrap();
        for kit in ["basic", "full", "recommended"] {
            assert_eq!(architect.default_on.get(kit).copied(), Some(false));
            architect.default_on.insert(kit.into(), true);
        }
        for (kit_id, kit) in &manifest.kits {
            assert!(
                !kit.components
                    .iter()
                    .any(|id| { id == "architect" || id == "litematica" || id == "malilib" }),
                "{kit_id}"
            );
        }
        let table = manifest.quality_presets.clone().unwrap();
        for row in [&table.potato, &table.lite, &table.normal, &table.ultra] {
            assert!(!row
                .skip_components
                .iter()
                .any(|id| { id == "architect" || id == "litematica" || id == "malilib" }));
        }
        let saved =
            selection_from(r#"{"kit":"full","ocean":true,"snowLite":false,"quality":"ultra"}"#);
        assert!(saved.addons.is_empty());
        let plan = build_plan(&manifest, &saved, None, true).unwrap();
        assert!(plan.components.contains(&"wakes".into()));
        assert!(!plan.components.contains(&"architect".into()));
        assert!(!plan.components.contains(&"litematica".into()));
        assert!(!plan.components.contains(&"malilib".into()));
    }

    #[test]
    fn kits_and_quality_do_not_turn_architect_on() {
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
                        .any(|id| { id == "architect" || id == "litematica" || id == "malilib" }),
                    "{kit} {quality:?} {components:?}",
                    components = plan.components
                );
            }
        }
    }

    #[test]
    fn architect_opt_in_installs_helper_and_deps() {
        let manifest = load_fixture_manifest().unwrap();
        let mut addons = BTreeMap::new();
        addons.insert("architect".into(), false);
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
        assert!(!off.components.contains(&"architect".into()));

        addons.insert("architect".into(), true);
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
        assert!(plan.components.contains(&"architect".into()));
        assert!(plan.components.contains(&"litematica".into()));
        assert!(plan.components.contains(&"malilib".into()));
        assert!(plan.components.contains(&"base".into()));
        assert!(!plan.components.contains(&"particlerain".into()));
        let architect_at = plan
            .components
            .iter()
            .position(|id| id == "architect")
            .unwrap();
        let litematica_at = plan
            .components
            .iter()
            .position(|id| id == "litematica")
            .unwrap();
        let malilib_at = plan
            .components
            .iter()
            .position(|id| id == "malilib")
            .unwrap();
        let base_at = plan.components.iter().position(|id| id == "base").unwrap();
        assert!(base_at < malilib_at && malilib_at < litematica_at && litematica_at < architect_at);
    }

    #[test]
    fn basic_kit_installs_midnightlib_without_coolrain() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                addons: BTreeMap::new(),
                snow_lite: None,
                quality: None,
            },
            None,
            true,
        )
        .unwrap();
        for id in [
            "midnightlib",
            "terrainslabs",
            "architectury",
            "lithium",
            "clumps",
            "dietplus",
            "glitchcore",
            "sereneseasons",
            "drinkbeer",
            "farmersdelight",
            "balm",
            "hardcorerevival",
            "base",
        ] {
            assert!(
                plan.components.iter().any(|item| item == id),
                "basic missing {id}"
            );
        }
        for id in ["coolrain", "flashback", "appleskin", "miniature"] {
            assert!(
                !plan.components.iter().any(|item| item == id),
                "basic has {id}"
            );
        }
    }

    const PACK_IDS: [&str; 6] = [
        "freshanimations",
        "mfrp",
        "matchablockoverlays",
        "bushypetals",
        "rainbowsfoliage",
        "grassrise",
    ];

    const PACK_FILES: [&str; 6] = [
        "FreshAnimations_v1.10.5.zip",
        "MFRP_1_12_7_26_3.zip",
        "Matcha-Block-Overlays-2+continuity.zip",
        "Bushy-pink-petals-wildflowers-leaf-litter.zip",
        "rainbows-foliage-polytone-edition-v2.0.zip",
        "Grassrise-1.0.3-mc26.3.zip",
    ];

    fn kit_plan(manifest: &Manifest, kit: &str) -> InstallPlan {
        build_plan(
            manifest,
            &Selection {
                kit: kit.into(),
                custom_components: vec![],
                ocean: false,
                addons: BTreeMap::new(),
                snow_lite: None,
                quality: None,
            },
            None,
            true,
        )
        .unwrap()
    }

    #[test]
    fn full_kit_plans_resource_packs_under_resourcepacks() {
        let manifest = load_fixture_manifest().unwrap();
        let full = kit_plan(&manifest, "full");
        for (id, filename) in PACK_IDS.iter().zip(PACK_FILES) {
            let file = full
                .add
                .iter()
                .find(|item| item.component_id == *id)
                .unwrap_or_else(|| panic!("full missing {id}"));
            assert_eq!(file.dest, FileDest::Resourcepacks);
            assert_eq!(file.file, filename);
            assert!(!file.file.contains(' '));
        }
        let basic = kit_plan(&manifest, "basic");
        for id in PACK_IDS {
            assert!(
                !basic.components.iter().any(|item| item == id),
                "basic has {id}"
            );
        }
        let recommended = kit_plan(&manifest, "recommended");
        for id in PACK_IDS {
            assert!(
                recommended.components.iter().any(|item| item == id),
                "recommended missing {id}"
            );
        }
    }

    #[test]
    fn resource_pack_state_diff_uses_the_resourcepacks_path() {
        let manifest = load_fixture_manifest().unwrap();
        let sha512 = manifest
            .components
            .iter()
            .find(|component| component.id == "freshanimations")
            .unwrap()
            .files[0]
            .sha512
            .clone();
        let selection = Selection {
            kit: "full".into(),
            custom_components: vec![],
            ocean: false,
            addons: BTreeMap::new(),
            snow_lite: None,
            quality: None,
        };
        let mut present = InstalledState::new("installer", "old");
        record_file(
            &mut present,
            "resourcepacks/FreshAnimations_v1.10.5.zip",
            &sha512,
            "freshanimations",
        );
        let same = build_plan(&manifest, &selection, Some(&present), true).unwrap();
        assert!(!same
            .add
            .iter()
            .any(|file| file.component_id == "freshanimations"));
        assert!(!same
            .update
            .iter()
            .any(|file| file.component_id == "freshanimations"));

        let mut wrong_folder = InstalledState::new("installer", "old");
        record_file(
            &mut wrong_folder,
            "mods/FreshAnimations_v1.10.5.zip",
            &sha512,
            "freshanimations",
        );
        let added = build_plan(&manifest, &selection, Some(&wrong_folder), true).unwrap();
        assert!(added
            .add
            .iter()
            .any(|file| file.component_id == "freshanimations"));

        let mut stale = InstalledState::new("installer", "old");
        record_file(&mut stale, "resourcepacks/old.zip", "x", "gonepack");
        let removed = build_plan(&manifest, &selection, Some(&stale), true).unwrap();
        let old = removed
            .remove
            .iter()
            .find(|file| file.file == "resourcepacks/old.zip")
            .expect("old pack removed");
        assert_eq!(old.dest, FileDest::Resourcepacks);
    }

    #[test]
    fn custom_resource_pack_pulls_its_mod_requires() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &Selection {
                kit: "custom".into(),
                custom_components: vec!["matchablockoverlays".into(), "rainbowsfoliage".into()],
                ocean: false,
                addons: BTreeMap::new(),
                snow_lite: None,
                quality: None,
            },
            None,
            true,
        )
        .unwrap();
        assert!(plan.components.contains(&"continuity".into()));
        assert!(plan.components.contains(&"polytone".into()));
        assert!(plan.components.contains(&"matchablockoverlays".into()));
        assert!(plan.components.contains(&"rainbowsfoliage".into()));
    }

    fn addon_selection(kit: &str, addon: Option<&str>, custom: &[&str]) -> Selection {
        let mut addons = BTreeMap::new();
        if let Some(addon) = addon {
            addons.insert(addon.to_string(), true);
        }
        Selection {
            kit: kit.into(),
            custom_components: custom.iter().map(|id| (*id).to_string()).collect(),
            ocean: false,
            addons,
            snow_lite: None,
            quality: None,
        }
    }

    #[test]
    fn recommended_flashback_is_not_optional() {
        let manifest = load_fixture_manifest().unwrap();
        for kit in ["recommended", "full"] {
            let plan = build_plan(&manifest, &addon_selection(kit, None, &[]), None, true).unwrap();
            assert!(plan.components.iter().any(|id| id == "flashback"), "{kit}");
            assert!(
                !plan.optional_components.iter().any(|id| id == "flashback"),
                "{kit} {:?}",
                plan.optional_components
            );
        }
    }

    #[test]
    fn architect_addon_on_recommended_is_optional() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &addon_selection("recommended", Some("architect"), &[]),
            None,
            true,
        )
        .unwrap();
        assert_eq!(
            plan.optional_components,
            vec![
                "malilib".to_string(),
                "litematica".to_string(),
                "architect".to_string()
            ]
        );
    }

    #[test]
    fn custom_direct_pick_of_malilib_is_not_optional_even_with_architect_addon() {
        let manifest = load_fixture_manifest().unwrap();
        let plan = build_plan(
            &manifest,
            &addon_selection("custom", Some("architect"), &["malilib"]),
            None,
            true,
        )
        .unwrap();
        assert!(!plan.optional_components.iter().any(|id| id == "malilib"));
        assert!(plan.optional_components.iter().any(|id| id == "litematica"));
        assert!(plan.optional_components.iter().any(|id| id == "architect"));
        assert!(plan.components.iter().any(|id| id == "malilib"));
    }

    #[test]
    fn ocean_addon_on_basic_marks_particlerain_and_wakes_optional() {
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
            true,
        )
        .unwrap();
        assert_eq!(
            plan.optional_components,
            vec!["particlerain".to_string(), "wakes".to_string()]
        );
        assert!(!plan.components.iter().any(|id| id == "waves"));
        assert!(!plan.optional_components.iter().any(|id| id == "waves"));
    }

    #[test]
    fn plan_for_an_existing_install_does_not_apply_to_an_empty_folder() {
        let manifest = load_fixture_manifest().unwrap();
        let selection = Selection {
            kit: "basic".into(),
            custom_components: vec![],
            ocean: false,
            addons: BTreeMap::new(),
            snow_lite: Some(true),
            quality: None,
        };
        let fresh = build_plan(&manifest, &selection, None, true).unwrap();
        let mut installed = InstalledState::new("installer", &manifest.manifest_version);
        for file in &fresh.add {
            record_file(
                &mut installed,
                format!("{}/{}", file.dest.folder(), file.file),
                &file.sha512,
                &file.component_id,
            );
        }
        let mut stale = build_plan(&manifest, &selection, Some(&installed), true).unwrap();
        assert!(stale.add.is_empty());
        stale.pack_id = "better-adventures-plus".into();
        stale.pack_dir = "/old/install".into();
        let wrong_folder = ensure_plan_current(
            &stale,
            "better-adventures-plus",
            Path::new("/new/empty"),
            &manifest,
            None,
        )
        .unwrap_err();
        assert_eq!(wrong_folder.code(), "E-STATE");

        let mut current = fresh;
        current.pack_id = "better-adventures-plus".into();
        current.pack_dir = "/new/empty".into();
        ensure_plan_current(
            &current,
            "better-adventures-plus",
            Path::new("/new/empty"),
            &manifest,
            None,
        )
        .unwrap();

        stale.pack_dir = "/new/empty".into();
        assert!(ensure_plan_current(
            &stale,
            "better-adventures-plus",
            Path::new("/new/empty"),
            &manifest,
            None,
        )
        .is_err());
        assert!(ensure_plan_current(
            &current,
            "other-pack",
            Path::new("/new/empty"),
            &manifest,
            None,
        )
        .is_err());
    }
}
