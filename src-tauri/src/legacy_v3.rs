//! Schema the public 3.0.x app (main@6362dfe) uses to accept a mod list.
//!
//! That build deserializes `group` into a four-value enum and rejects any
//! component id that is not lowercase letters and digits. A newer group or an
//! underscore in an id fails the whole manifest, and every installed copy
//! falls back to its cache or the bundled fixture.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::download::{is_allowed_loader_url, is_allowed_source_url};
use crate::manifest::{
    Addon, ComponentFile, ComponentKind, Kit, Loader, BUNDLED_MANIFEST_JSON, MANIFEST_SCHEMA,
};
use crate::quality::QualityTable;

/// `ComponentGroup` from main@6362dfe. No `sao` variant.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum LegacyGroup {
    Base,
    Shaders,
    Arnold,
    Ocean,
}

/// Component id rule from main@6362dfe. Underscores are rejected.
fn legacy_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }
        _ => false,
    }
}

/// Match-glob rule from main@6362dfe. File names may contain `_` and `+`.
fn legacy_valid_match(pattern: &str) -> bool {
    if pattern.contains('/') || pattern.contains('\\') {
        return false;
    }
    let Some((stem, ext)) = pattern.rsplit_once('.') else {
        return false;
    };
    if ext != "jar" && ext != "zip" {
        return false;
    }
    let mut cs = stem.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') => {
            cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '*'))
        }
        _ => false,
    }
}

fn legacy_valid_sha512(value: &str) -> bool {
    value.len() == 128 && value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyComponent {
    id: String,
    group: LegacyGroup,
    kind: ComponentKind,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    required_in_all_kits: bool,
    name: String,
    version: String,
    description: String,
    #[serde(default)]
    legacy_match: Vec<String>,
    #[serde(default)]
    known_sha512: Vec<String>,
    files: Vec<ComponentFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyManifest {
    schema: u32,
    manifest_version: String,
    min_app_version: String,
    minecraft: String,
    loader: Loader,
    components: Vec<LegacyComponent>,
    kits: BTreeMap<String, Kit>,
    #[serde(default)]
    addons: BTreeMap<String, Addon>,
    #[serde(default)]
    quality_presets: Option<QualityTable>,
}

/// Selection JSON the 3.0.x app sends. Unknown keys, including `addons`, are ignored.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySelection {
    kit: String,
    #[serde(default)]
    custom_components: Vec<String>,
    #[serde(default)]
    ocean: bool,
}

/// Groups the 3.0.x Custom screen renders. `base` is a valid group and is not listed.
const LEGACY_CUSTOM_GROUPS: &[&str] = &["arnold", "shaders", "ocean"];

const LEGACY_CUSTOM_DEFAULTS: &[&str] = &["deck", "ships", "snow", "sodium", "base"];

fn parse_legacy(bytes: &str) -> Result<LegacyManifest, String> {
    let manifest: LegacyManifest = serde_json::from_str(bytes).map_err(|err| err.to_string())?;
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(format!("schema {}", manifest.schema));
    }
    if manifest.manifest_version.trim().is_empty()
        || manifest.min_app_version.trim().is_empty()
        || manifest.minecraft.trim().is_empty()
    {
        return Err("required string field is empty".into());
    }
    if !legacy_valid_sha512(&manifest.loader.sha512) {
        return Err("loader.sha512".into());
    }
    if !is_allowed_loader_url(&manifest.loader.url) {
        return Err(format!("loader url {}", manifest.loader.url));
    }
    let ids: BTreeSet<&str> = manifest
        .components
        .iter()
        .map(|component| component.id.as_str())
        .collect();
    for component in &manifest.components {
        if !legacy_valid_id(&component.id) {
            return Err(format!("invalid component id {}", component.id));
        }
        if component.kind != ComponentKind::Pending && component.files.is_empty() {
            return Err(format!("component {} has no files", component.id));
        }
        for file in &component.files {
            if !legacy_valid_sha512(&file.sha512) {
                return Err(format!("bad sha512 on {}", file.file));
            }
            if !legacy_valid_match(&file.match_glob) {
                return Err(format!("bad match glob {}", file.match_glob));
            }
            if !is_allowed_source_url(&file.url) {
                return Err(format!("url {}", file.url));
            }
        }
        for extra in &component.legacy_match {
            if !legacy_valid_match(extra) {
                return Err(format!("bad legacy match glob {extra}"));
            }
        }
        for known in &component.known_sha512 {
            if !legacy_valid_sha512(known) {
                return Err(format!("bad knownSha512 on {}", component.id));
            }
        }
        for req in &component.requires {
            if !ids.contains(req.as_str()) {
                return Err(format!("{} requires unknown {req}", component.id));
            }
        }
    }
    for (kit_id, kit) in &manifest.kits {
        for id in &kit.components {
            if !ids.contains(id.as_str()) {
                return Err(format!("kit {kit_id} references unknown component {id}"));
            }
        }
    }
    for (addon_id, addon) in &manifest.addons {
        for id in &addon.components {
            if !ids.contains(id.as_str()) {
                return Err(format!(
                    "addon {addon_id} references unknown component {id}"
                ));
            }
        }
    }
    Ok(manifest)
}

fn group_name(group: LegacyGroup) -> &'static str {
    match group {
        LegacyGroup::Base => "base",
        LegacyGroup::Shaders => "shaders",
        LegacyGroup::Arnold => "arnold",
        LegacyGroup::Ocean => "ocean",
    }
}

/// What the 3.0.x planner selects. It reads the `ocean` addon only.
fn legacy_select(
    manifest: &LegacyManifest,
    kit: &str,
    ocean: bool,
    custom: &[String],
    skip: &[String],
) -> Vec<String> {
    let mut selected = if kit == "custom" {
        custom.to_vec()
    } else {
        manifest
            .kits
            .get(kit)
            .map(|entry| entry.components.clone())
            .unwrap_or_default()
    };
    for component in &manifest.components {
        if component.required_in_all_kits && !selected.iter().any(|id| id == &component.id) {
            selected.push(component.id.clone());
        }
    }
    if ocean {
        if let Some(addon) = manifest.addons.get("ocean") {
            for id in &addon.components {
                if skip.iter().any(|skipped| skipped == id) {
                    continue;
                }
                let Some(component) = manifest.components.iter().find(|item| &item.id == id) else {
                    continue;
                };
                if component.kind != ComponentKind::Pending && !selected.contains(id) {
                    selected.push(id.clone());
                }
            }
        }
    }
    selected.retain(|id| {
        !skip.iter().any(|skipped| skipped == id)
            && manifest
                .components
                .iter()
                .find(|component| &component.id == id)
                .is_some_and(|component| component.kind != ComponentKind::Pending)
    });
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    fn walk(
        id: &str,
        manifest: &LegacyManifest,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) {
        if !seen.insert(id.to_string()) {
            return;
        }
        let Some(component) = manifest.components.iter().find(|item| item.id == id) else {
            return;
        };
        for req in &component.requires {
            walk(req, manifest, seen, out);
        }
        out.push(id.to_string());
    }
    for id in &selected {
        walk(id, manifest, &mut seen, &mut out);
    }
    out
}

fn assert_staging_absent(ids: &[String], kit: &str, ocean: bool) {
    assert!(
        !ids.iter().any(|id| id == "saoaincrad" || id == "geckolib"),
        "{kit} ocean={ocean} selected {ids:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_parses_on_the_6362dfe_schema_and_leaves_sao_off() {
        let manifest = parse_legacy(BUNDLED_MANIFEST_JSON).expect("6362dfe schema");
        assert_eq!(manifest.min_app_version, "3.0.0");
        assert_eq!(manifest.minecraft, "26.3");
        assert!(manifest.addons.contains_key("sao"));
        assert!(manifest.addons.contains_key("ocean"));

        let sao = manifest
            .components
            .iter()
            .find(|component| component.id == "saoaincrad")
            .expect("sao component");
        let gecko = manifest
            .components
            .iter()
            .find(|component| component.id == "geckolib")
            .expect("geckolib");
        assert_eq!(sao.group, LegacyGroup::Base);
        assert_eq!(gecko.group, LegacyGroup::Base);
        assert!(!sao.required_in_all_kits);
        assert!(!gecko.required_in_all_kits);
        assert_eq!(sao.name, "SAO: Aincrad (staging test)");
        assert_eq!(gecko.name, "GeckoLib");
        assert!(sao.description.contains("Staging test only"));
        assert_eq!(sao.version, "0.0.3-spike+1c25315");

        let visible: Vec<&str> = manifest
            .components
            .iter()
            .filter(|component| LEGACY_CUSTOM_GROUPS.contains(&group_name(component.group)))
            .map(|component| component.id.as_str())
            .collect();
        assert!(!visible.contains(&"saoaincrad"));
        assert!(!visible.contains(&"geckolib"));

        let table = manifest.quality_presets.as_ref().expect("presets");
        let presets = [
            ("potato", &table.potato),
            ("lite", &table.lite),
            ("normal", &table.normal),
            ("ultra", &table.ultra),
        ];
        for (level, row) in presets {
            assert!(
                !row.skip_components
                    .iter()
                    .any(|id| id == "saoaincrad" || id == "geckolib"),
                "{level}"
            );
            for kit in ["basic", "full", "recommended"] {
                let ids = legacy_select(
                    &manifest,
                    kit,
                    row.recommend_ocean,
                    &[],
                    &row.skip_components,
                );
                assert_staging_absent(&ids, kit, row.recommend_ocean);
            }
        }

        let defaults: Vec<String> = LEGACY_CUSTOM_DEFAULTS
            .iter()
            .map(|id| (*id).to_string())
            .collect();
        for ocean in [false, true] {
            let ids = legacy_select(&manifest, "custom", ocean, &defaults, &[]);
            assert_staging_absent(&ids, "custom", ocean);
            assert!(ids.contains(&"base".to_string()));
            assert!(!ids.contains(&"shaders".to_string()));
        }

        let saved: LegacySelection = serde_json::from_str(
            r#"{"kit":"full","ocean":true,"snowLite":false,"quality":"ultra","addons":{"sao":true}}"#,
        )
        .unwrap();
        assert!(saved.ocean);
        assert!(saved.custom_components.is_empty());
        let ids = legacy_select(
            &manifest,
            &saved.kit,
            saved.ocean,
            &saved.custom_components,
            &[],
        );
        assert_staging_absent(&ids, "saved-full", true);
        assert!(ids.contains(&"particlerain".to_string()));
    }

    #[test]
    fn legacy_schema_rejects_a_new_group_and_an_underscore_id() {
        assert!(legacy_valid_id("geckolib"));
        assert!(legacy_valid_id("saoaincrad"));
        assert!(!legacy_valid_id("sao_aincrad"));
        let err = serde_json::from_str::<LegacyGroup>("\"sao\"").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("sao") || message.contains("variant"),
            "{message}"
        );
        let mut value: serde_json::Value = serde_json::from_str(BUNDLED_MANIFEST_JSON).unwrap();
        value["components"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|component| component["id"] == "saoaincrad")
            .unwrap()["group"] = serde_json::json!("sao");
        let err = parse_legacy(&value.to_string()).unwrap_err();
        assert!(err.contains("sao") || err.contains("variant"), "{err}");
        value["components"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|component| component["group"] == "sao")
            .unwrap()["id"] = serde_json::json!("sao_aincrad");
        value["components"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|component| component["id"] == "sao_aincrad")
            .unwrap()["group"] = serde_json::json!("base");
        let err = parse_legacy(&value.to_string()).unwrap_err();
        assert!(err.contains("sao_aincrad"), "{err}");
    }
}
