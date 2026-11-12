use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub title: String,
    pub hint: String,
    pub detail: String,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Minecraft {version} is not installed")]
    McMissing { version: String, path: String },
    #[error("URL is not on the download allowlist: {url}")]
    UrlNotAllowed { url: String },
    #[error("Manifest signature is invalid")]
    SignatureInvalid { detail: String },
    #[error("SHA-512 mismatch")]
    HashMismatch {
        expected: String,
        actual: String,
        what: String,
    },
    #[error("Minecraft appears to be running")]
    GameRunning,
    #[error("Manifest schema error: {0}")]
    ManifestSchema(String),
    #[error("Manifest requires error: {0}")]
    ManifestRequires(String),
    #[error("A different file is already in that slot")]
    Collision { path: String, detail: String },
    #[error("I/O error: {0}")]
    Io(String),
    #[error("State error: {0}")]
    State(String),
    #[error("Snapshot error: {0}")]
    Snapshot(String),
    #[error("Install was cancelled")]
    Cancelled,
    #[error("{what} is not implemented in this build")]
    NotImplemented { what: String },
    #[error("{title}")]
    Other {
        code: String,
        title: String,
        hint: String,
        detail: String,
    },
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::ManifestSchema(value.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(value: zip::result::ZipError) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<hex::FromHexError> for AppError {
    fn from(value: hex::FromHexError) -> Self {
        Self::SignatureInvalid {
            detail: format!("hex decode: {value}"),
        }
    }
}

impl AppError {
    pub fn code(&self) -> String {
        match self {
            Self::McMissing { .. } => "E-MC-MISSING".to_string(),
            Self::UrlNotAllowed { .. } => "E-URL-NOT-ALLOWED".to_string(),
            Self::SignatureInvalid { .. } => "E-SIG-INVALID".to_string(),
            Self::HashMismatch { .. } => "E-HASH-MISMATCH".to_string(),
            Self::GameRunning => "E-GAME-RUNNING".to_string(),
            Self::ManifestSchema(_) => "E-MANIFEST-SCHEMA".to_string(),
            Self::ManifestRequires(_) => "E-MANIFEST-REQUIRES".to_string(),
            Self::Collision { .. } => "P-COLLISION".to_string(),
            Self::Io(_) => "E-IO".to_string(),
            Self::State(_) => "E-STATE".to_string(),
            Self::Snapshot(_) => "E-SNAPSHOT".to_string(),
            Self::Cancelled => "E-CANCELLED".to_string(),
            Self::NotImplemented { .. } => "E-NOT-IMPLEMENTED".to_string(),
            Self::Other { code, .. } => code.clone(),
        }
    }

    pub fn to_payload(&self) -> ErrorPayload {
        match self {
            Self::McMissing { version, path } => ErrorPayload {
                code: self.code(),
                title: format!("Minecraft {version} isn't installed yet"),
                hint: format!(
                    "Open the Minecraft Launcher, press Play once on Latest release ({version}), close the game, then press Try again."
                ),
                detail: format!("detected: versions/{version}/{version}.jar not found in {path}"),
            },
            Self::UrlNotAllowed { url } => ErrorPayload {
                code: self.code(),
                title: "That download address is not allowed".to_string(),
                hint: "The installer only fetches from cdn.modrinth.com, this project's GitHub Releases, Fabric meta for the loader profile, and The Architect's official download site.".to_string(),
                detail: url.clone(),
            },
            Self::SignatureInvalid { detail } => ErrorPayload {
                code: self.code(),
                title: "The mod list signature does not match".to_string(),
                hint: "Nothing was changed. Try again when you are online, or keep using the last good list.".to_string(),
                detail: detail.clone(),
            },
            Self::HashMismatch {
                expected,
                actual,
                what,
            } => ErrorPayload {
                code: self.code(),
                title: "A file failed its SHA-512 check".to_string(),
                hint: "The download was discarded. Your game folder was not changed.".to_string(),
                detail: format!("{what}: expected {expected}, got {actual}"),
            },
            Self::GameRunning => ErrorPayload {
                code: self.code(),
                title: "Minecraft is still running".to_string(),
                hint: "Close the game and the launcher, then try again. Locked jars cannot be replaced on Windows.".to_string(),
                detail: "A Minecraft or Fabric Java process is running.".to_string(),
            },
            Self::ManifestSchema(detail) => ErrorPayload {
                code: self.code(),
                title: "The mod list is not valid".to_string(),
                hint: "Wait for a new mods release. Nothing was changed on your PC.".to_string(),
                detail: detail.clone(),
            },
            Self::ManifestRequires(detail) => ErrorPayload {
                code: self.code(),
                title: "The mod list has a broken dependency".to_string(),
                hint: "Wait for a new mods release. Nothing was changed on your PC.".to_string(),
                detail: detail.clone(),
            },
            Self::Collision { path, detail } => ErrorPayload {
                code: self.code(),
                title: format!("A different {path} is already in your mods folder"),
                hint: "Replace it (the old one is kept in the restore point), keep yours and skip, or cancel. Unknown snow jars are left alone until you choose.".to_string(),
                detail: detail.clone(),
            },
            Self::Io(detail) => ErrorPayload {
                code: self.code(),
                title: "The installer could not read or write a file".to_string(),
                hint: "Check that you can write to the game folder, then try again.".to_string(),
                detail: detail.clone(),
            },
            Self::State(detail) => ErrorPayload {
                code: self.code(),
                title: "The install record could not be read".to_string(),
                hint: "The installer can rebuild the record from what it finds, or you can pick the game folder again.".to_string(),
                detail: detail.clone(),
            },
            Self::Snapshot(detail) => ErrorPayload {
                code: self.code(),
                title: "The restore point could not be used".to_string(),
                hint: "Your untracked files were left alone. Try another restore point.".to_string(),
                detail: detail.clone(),
            },
            Self::Cancelled => ErrorPayload {
                code: self.code(),
                title: "Install cancelled".to_string(),
                hint: "Nothing was left half-applied. Your game folder is as it was before this run.".to_string(),
                detail: "The install transaction rolled back cleanly.".to_string(),
            },
            Self::NotImplemented { what } => ErrorPayload {
                code: self.code(),
                title: format!("{what} is not in this build yet"),
                hint: "This optional piece is disabled in this build. Everything else still works.".to_string(),
                detail: what.clone(),
            },
            Self::Other {
                code,
                title,
                hint,
                detail,
            } => ErrorPayload {
                code: code.clone(),
                title: title.clone(),
                hint: hint.clone(),
                detail: detail.clone(),
            },
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_payload().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_missing_payload_matches_catalogue() {
        let err = AppError::McMissing {
            version: "26.3".into(),
            path: r"C:\Users\Sam\AppData\Roaming\.minecraft".into(),
        };
        let payload = err.to_payload();
        assert_eq!(payload.code, "E-MC-MISSING");
        assert!(payload.title.contains("26.3"));
        assert!(payload.detail.contains("26.3.jar"));
        assert!(payload.hint.contains("Play"));
    }

    #[test]
    fn url_error_code_is_stable() {
        let err = AppError::UrlNotAllowed {
            url: "https://evil.example/mod.jar".into(),
        };
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }

    #[test]
    fn cancelled_explains_clean_rollback() {
        let payload = AppError::Cancelled.to_payload();
        assert_eq!(payload.code, "E-CANCELLED");
        assert!(payload.hint.contains("half-applied"));
    }

    #[test]
    fn collision_asks_before_touching_unknown_snow() {
        let payload = AppError::Collision {
            path: "mods/arnold-snowfall-mystery.jar".into(),
            detail: "checksum is not a known Arnold build".into(),
        }
        .to_payload();
        assert_eq!(payload.code, "P-COLLISION");
        assert!(payload.hint.contains("Unknown snow"));
    }
}
