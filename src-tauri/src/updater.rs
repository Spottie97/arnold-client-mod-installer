use serde::{Deserialize, Serialize};

pub const LATEST_JSON_URL: &str =
    "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/latest.json";
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_json_is_on_project_releases() {
        assert!(LATEST_JSON_URL.contains("/releases/latest/download/latest.json"));
        assert!(LATEST_JSON_URL.contains("arnold-client-mod-installer"));
    }

    #[test]
    fn default_build_disables_updater() {
        let status = disabled_status();
        assert!(!status.available);
        assert!(!status.enabled);
        assert!(status.reason.contains("CI secret"));
    }
}
