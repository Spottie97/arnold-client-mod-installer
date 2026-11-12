use serde::{Deserialize, Serialize};

/// Stable feed URL. This must not use /releases/latest, which is shared with
/// the mods manifest and currently points at a mods-* tag that has no latest.json.
pub const LATEST_JSON_URL: &str =
    "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/app-updater/latest.json";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub available: bool,
    #[serde(default)]
    pub version: Option<String>,
    pub reason: String,
    pub endpoint: String,
    pub enabled: bool,
}

pub fn disabled_status() -> UpdateStatus {
    UpdateStatus {
        available: false,
        version: None,
        reason: "Updater is disabled in this build. The signing key is a CI secret and was not provided.".into(),
        endpoint: LATEST_JSON_URL.to_string(),
        enabled: false,
    }
}

pub fn updater_enabled() -> bool {
    cfg!(feature = "updater")
}

pub fn check_update_sync() -> UpdateStatus {
    disabled_status()
}

/// Public key injected at compile time from TAURI_UPDATER_PUBKEY (release builds
/// with --features updater). Empty in everyday builds that do not set the env.
pub fn injected_updater_pubkey() -> Option<&'static str> {
    let key = option_env!("TAURI_UPDATER_PUBKEY").unwrap_or("").trim();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

#[cfg(feature = "updater")]
pub fn configured_updater(app: &tauri::AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    use tauri_plugin_updater::UpdaterExt;

    let url = url::Url::parse(LATEST_JSON_URL).map_err(|e| e.to_string())?;
    let mut builder = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?;
    if let Some(key) = injected_updater_pubkey() {
        builder = builder.pubkey(key);
    }
    builder.build().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_json_uses_the_rolling_app_updater_tag() {
        assert!(LATEST_JSON_URL.contains("/releases/download/app-updater/latest.json"));
        assert!(LATEST_JSON_URL.contains("arnold-client-mod-installer"));
        assert!(
            !LATEST_JSON_URL.contains("/releases/latest/"),
            "the app updater must not share /releases/latest with the mods manifest"
        );
    }

    #[test]
    fn default_build_disables_updater() {
        let status = disabled_status();
        assert!(!status.available);
        assert!(!status.enabled);
        assert!(status.reason.contains("CI secret"));
        assert!(status
            .endpoint
            .contains("/releases/download/app-updater/latest.json"));
    }
}
