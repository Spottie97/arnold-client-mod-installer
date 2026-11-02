use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// First-line marker that means the installer still owns a lite snow file.
pub const LITE_MARKER: &str = "# arnold-installer-lite";
pub const MANAGED_HEADER: &str = "# arnold-installer-managed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSwitch {
    None,
    ToLite,
    ToNormal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertiesOutcome {
    pub content: String,
    pub skipped_switch: bool,
    pub added_keys: Vec<String>,
    pub rewritten: bool,
}

#[derive(Debug, Clone)]
struct ParsedLine {
    raw: String,
    ending: String,
    key: Option<String>,
}

fn detect_ending(existing: &str) -> &'static str {
    if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn parse_lines(existing: &str) -> Vec<ParsedLine> {
    if existing.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut rest = existing;
    while !rest.is_empty() {
        let (raw, ending, next) = if let Some(idx) = rest.find("\r\n") {
            (&rest[..idx], "\r\n".to_string(), &rest[idx + 2..])
        } else if let Some(idx) = rest.find('\n') {
            (&rest[..idx], "\n".to_string(), &rest[idx + 1..])
        } else {
            (rest, String::new(), "")
        };
        let key = property_key(raw);
        lines.push(ParsedLine {
            raw: raw.to_string(),
            ending,
            key,
        });
        rest = next;
    }
    lines
}

fn property_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }
    let eq = trimmed.find('=')?;
    let key = trimmed[..eq].trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn first_line_is_marker(existing: &str, marker: &str) -> bool {
    let first = existing
        .lines()
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .trim();
    first == marker
}

pub fn format_setting_value(value: &Value) -> String {
    match value {
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn write_file(header: &str, settings: &BTreeMap<String, String>, ending: &str) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push_str(ending);
    for (key, value) in settings {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push_str(ending);
    }
    out
}

fn add_missing_only(
    existing: &str,
    settings: &BTreeMap<String, String>,
    header_if_empty: &str,
) -> PropertiesOutcome {
    if existing.is_empty() {
        return PropertiesOutcome {
            content: write_file(header_if_empty, settings, "\n"),
            skipped_switch: false,
            added_keys: settings.keys().cloned().collect(),
            rewritten: true,
        };
    }

    let ending = detect_ending(existing).to_string();
    let lines = parse_lines(existing);
    let present: BTreeSet<String> = lines.iter().filter_map(|l| l.key.clone()).collect();
    let mut added_keys = Vec::new();
    let mut out = String::new();
    for line in &lines {
        out.push_str(&line.raw);
        out.push_str(&line.ending);
    }
    let needs_nl = !out.is_empty() && !out.ends_with('\n');
    if needs_nl {
        out.push_str(&ending);
    }
    for (key, value) in settings {
        if present.contains(key) {
            continue;
        }
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push_str(&ending);
        added_keys.push(key.clone());
    }
    PropertiesOutcome {
        content: out,
        skipped_switch: false,
        added_keys,
        rewritten: false,
    }
}

/// Merge Java `.properties` with the §12.2 add-only rule and lite-marker rewrite.
pub fn apply_properties(
    existing: Option<&str>,
    defaults: &BTreeMap<String, String>,
    lite_overrides: Option<&BTreeMap<String, String>>,
    header: &str,
    lite_marker: &str,
    switch: ConfigSwitch,
) -> PropertiesOutcome {
    let existing = existing.unwrap_or("");
    let mut lite_values = defaults.clone();
    if let Some(over) = lite_overrides {
        lite_values.extend(over.clone());
    }
    let managed = first_line_is_marker(existing, lite_marker);
    let ending = if existing.is_empty() {
        "\n"
    } else {
        detect_ending(existing)
    };

    match switch {
        ConfigSwitch::ToLite => {
            if existing.is_empty() || managed {
                return PropertiesOutcome {
                    content: write_file(lite_marker, &lite_values, ending),
                    skipped_switch: false,
                    added_keys: lite_values.keys().cloned().collect(),
                    rewritten: true,
                };
            }
            let mut out = add_missing_only(existing, &lite_values, header);
            out.skipped_switch = true;
            out
        }
        ConfigSwitch::ToNormal => {
            if managed {
                return PropertiesOutcome {
                    content: write_file(header, defaults, ending),
                    skipped_switch: false,
                    added_keys: defaults.keys().cloned().collect(),
                    rewritten: true,
                };
            }
            let mut out = add_missing_only(existing, defaults, header);
            if !existing.is_empty() {
                out.skipped_switch = true;
            }
            out
        }
        ConfigSwitch::None => {
            if existing.is_empty() {
                add_missing_only("", defaults, header)
            } else {
                add_missing_only(existing, defaults, header)
            }
        }
    }
}

/// Player-owned settings screen: overwrite listed keys, keep the rest.
pub fn update_property_keys(existing: &str, updates: &BTreeMap<String, String>) -> String {
    if existing.is_empty() {
        let mut out = String::new();
        for (key, value) in updates {
            out.push_str(key);
            out.push('=');
            out.push_str(value);
            out.push('\n');
        }
        return out;
    }
    let ending = detect_ending(existing).to_string();
    let lines = parse_lines(existing);
    let mut seen = BTreeSet::new();
    let mut out = String::new();
    for line in &lines {
        if let Some(key) = &line.key {
            if let Some(value) = updates.get(key) {
                out.push_str(key);
                out.push('=');
                out.push_str(value);
                out.push_str(&line.ending);
                seen.insert(key.clone());
                continue;
            }
        }
        out.push_str(&line.raw);
        out.push_str(&line.ending);
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(&ending);
    }
    for (key, value) in updates {
        if seen.contains(key) {
            continue;
        }
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push_str(&ending);
    }
    out
}

pub fn values_that_differ(
    existing: &str,
    defaults: &BTreeMap<String, String>,
) -> Vec<(String, String, String)> {
    let lines = parse_lines(existing);
    let mut found = BTreeMap::new();
    for line in lines {
        if let Some(key) = line.key {
            if let Some(eq) = line.raw.find('=') {
                let value = line.raw[eq + 1..].trim().to_string();
                found.insert(key, value);
            }
        }
    }
    let mut diffs = Vec::new();
    for (key, default) in defaults {
        if let Some(current) = found.get(key) {
            if current != default {
                diffs.push((key.clone(), current.clone(), default.clone()));
            }
        }
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn snow_defaults() -> BTreeMap<String, String> {
        map(&[
            ("denserSnow", "true"),
            ("density", "24"),
            ("treeCaps", "true"),
            ("crunch", "true"),
        ])
    }

    fn snow_lite() -> BTreeMap<String, String> {
        map(&[("density", "8"), ("treeCaps", "false")])
    }

    #[test]
    fn keeps_existing_values_comments_order_and_adds_missing() {
        let existing = "# player note\ndenserSnow=false\n! keep me\ndensity=99\n";
        let out = apply_properties(
            Some(existing),
            &snow_defaults(),
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        );
        assert!(!out.rewritten);
        assert_eq!(
            out.added_keys,
            vec!["crunch".to_string(), "treeCaps".to_string()]
        );
        assert!(out.content.starts_with("# player note\n"));
        assert!(out.content.contains("denserSnow=false\n"));
        assert!(out.content.contains("! keep me\n"));
        assert!(out.content.contains("density=99\n"));
        assert!(out.content.contains("treeCaps=true\n"));
        assert!(out.content.contains("crunch=true\n"));
        let denser_pos = out.content.find("denserSnow=false").unwrap();
        let density_pos = out.content.find("density=99").unwrap();
        assert!(denser_pos < density_pos);
    }

    #[test]
    fn keeps_crlf() {
        let existing = "denserSnow=false\r\ndensity=1\r\n";
        let out = apply_properties(
            Some(existing),
            &snow_defaults(),
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        );
        assert!(out.content.contains("\r\n"));
        assert!(!out.content.contains("false\n"));
        assert!(out.content.contains("treeCaps=true\r\n"));
    }

    #[test]
    fn creates_new_file_with_header() {
        let out = apply_properties(
            None,
            &snow_defaults(),
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        );
        assert!(out.rewritten);
        assert!(out.content.starts_with(&format!("{MANAGED_HEADER}\n")));
        assert!(out.content.contains("denserSnow=true\n"));
    }

    #[test]
    fn lite_rewrites_when_marker_present() {
        let existing = format!("{LITE_MARKER}\ndensity=3\ntreeCaps=true\n");
        let out = apply_properties(
            Some(&existing),
            &snow_defaults(),
            Some(&snow_lite()),
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::ToLite,
        );
        assert!(out.rewritten);
        assert!(!out.skipped_switch);
        assert!(out.content.starts_with(&format!("{LITE_MARKER}\n")));
        assert!(out.content.contains("density=8\n"));
        assert!(out.content.contains("treeCaps=false\n"));
    }

    #[test]
    fn lite_creates_when_missing() {
        let out = apply_properties(
            None,
            &snow_defaults(),
            Some(&snow_lite()),
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::ToLite,
        );
        assert!(out.rewritten);
        assert!(out.content.starts_with(LITE_MARKER));
        assert!(out.content.contains("density=8\n"));
    }

    #[test]
    fn lite_switch_skipped_when_marker_removed() {
        let existing = "denserSnow=false\ndensity=99\n";
        let out = apply_properties(
            Some(existing),
            &snow_defaults(),
            Some(&snow_lite()),
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::ToLite,
        );
        assert!(out.skipped_switch);
        assert!(!out.rewritten);
        assert!(out.content.contains("denserSnow=false\n"));
        assert!(out.content.contains("density=99\n"));
        assert!(out.content.contains("treeCaps=false\n"));
    }

    #[test]
    fn normal_switch_rewrites_marked_file() {
        let existing = format!("{LITE_MARKER}\ndensity=8\n");
        let out = apply_properties(
            Some(&existing),
            &snow_defaults(),
            Some(&snow_lite()),
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::ToNormal,
        );
        assert!(out.rewritten);
        assert!(out.content.starts_with(MANAGED_HEADER));
        assert!(out.content.contains("density=24\n"));
        assert!(!out.content.contains(LITE_MARKER));
    }

    #[test]
    fn normal_switch_skipped_when_player_owns_file() {
        let existing = "density=7\n";
        let out = apply_properties(
            Some(existing),
            &snow_defaults(),
            Some(&snow_lite()),
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::ToNormal,
        );
        assert!(out.skipped_switch);
        assert!(out.content.contains("density=7\n"));
    }

    #[test]
    fn reports_values_that_differ_from_defaults() {
        let existing = "density=7\ntreeCaps=true\n";
        let diffs = values_that_differ(existing, &snow_defaults());
        assert_eq!(diffs, vec![("density".into(), "7".into(), "24".into())]);
    }

    #[test]
    fn settings_screen_overwrites_listed_keys_only() {
        let existing = "# note\ndensity=7\ntreeCaps=true\n";
        let updated = update_property_keys(existing, &map(&[("density", "8")]));
        assert!(updated.contains("# note\n"));
        assert!(updated.contains("density=8\n"));
        assert!(updated.contains("treeCaps=true\n"));
    }
}
