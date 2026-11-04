use sha2::{Digest, Sha512};
use url::Url;

use crate::errors::AppError;

/// GitHub owner/repo whose Releases may be fetched. Fixed for this app.
pub const RELEASE_OWNER: &str = "Spottie97";
pub const RELEASE_REPO: &str = "arnold-client-mod-installer";
pub const MODRINTH_HOST: &str = "cdn.modrinth.com";
pub const GITHUB_HOST: &str = "github.com";
pub const FABRIC_META_HOST: &str = "meta.fabricmc.net";
pub const ARNOLD_RELEASE_TAG: &str = "mods-2026.11.5";

const GITHUB_ASSET_HOSTS: &[&str] = &[
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedSource {
    Modrinth,
    GitHubReleases,
    FabricMeta,
}

fn reject(url: &str) -> AppError {
    AppError::UrlNotAllowed {
        url: url.to_string(),
    }
}

fn parsed_https(raw: &str) -> Result<Url, AppError> {
    let trimmed = raw.trim();
    if trimmed != raw
        || raw
            .as_bytes()
            .iter()
            .any(|b| b.is_ascii_whitespace() || *b == 0)
    {
        return Err(reject(raw));
    }
    if raw.contains('\\') || raw.contains("..") {
        return Err(reject(raw));
    }
    let url = Url::parse(raw).map_err(|_| reject(raw))?;
    if url.scheme() != "https" {
        return Err(reject(raw));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(reject(raw));
    }
    if url.port().is_some() {
        return Err(reject(raw));
    }
    if url.host_str().is_none() {
        return Err(reject(raw));
    }
    Ok(url)
}

fn host_of(url: &Url) -> String {
    url.host_str().unwrap_or("").to_ascii_lowercase()
}

/// Allow only `https://cdn.modrinth.com/…` and
/// `https://github.com/Spottie97/arnold-client-mod-installer/releases/…`.
pub fn classify_source_url(raw: &str) -> Result<AllowedSource, AppError> {
    let url = parsed_https(raw)?;
    let host = host_of(&url);
    let path = url.path();

    if host == MODRINTH_HOST {
        if !path.starts_with('/') || path.contains("..") {
            return Err(reject(raw));
        }
        return Ok(AllowedSource::Modrinth);
    }

    if host == GITHUB_HOST {
        let prefix = format!("/{RELEASE_OWNER}/{RELEASE_REPO}/releases");
        let allowed = path == prefix || path.starts_with(&format!("{prefix}/"));
        if allowed && !path.contains("..") {
            return Ok(AllowedSource::GitHubReleases);
        }
    }

    Err(reject(raw))
}

/// Loader-only extra host: `https://meta.fabricmc.net/v2/versions/loader/…`
pub fn classify_loader_url(raw: &str) -> Result<AllowedSource, AppError> {
    if let Ok(source) = classify_source_url(raw) {
        return Ok(source);
    }
    let url = parsed_https(raw)?;
    let host = host_of(&url);
    let path = url.path();
    if host == FABRIC_META_HOST
        && (path == "/v2/versions/loader" || path.starts_with("/v2/versions/loader/"))
        && !path.contains("..")
    {
        return Ok(AllowedSource::FabricMeta);
    }
    Err(reject(raw))
}

pub fn is_allowed_source_url(raw: &str) -> bool {
    classify_source_url(raw).is_ok()
}

pub fn is_allowed_loader_url(raw: &str) -> bool {
    classify_loader_url(raw).is_ok()
}

pub fn arnold_release_asset_url(file_name: &str) -> String {
    format!(
        "https://{GITHUB_HOST}/{RELEASE_OWNER}/{RELEASE_REPO}/releases/download/{ARNOLD_RELEASE_TAG}/{file_name}"
    )
}

/// Redirects are followed only to GitHub release asset hosts, and only from an
/// already-allowlisted origin URL. SHA-512 is still checked afterwards.
pub fn is_allowed_redirect(from: &str, to: &str) -> Result<(), AppError> {
    classify_source_url(from)?;
    let dest = parsed_https(to)?;
    let host = host_of(&dest);
    if GITHUB_ASSET_HOSTS.contains(&host.as_str()) {
        return Ok(());
    }
    // `releases/latest/download/<file>` first redirects to
    // `releases/download/<tag>/<file>` on github.com, still inside this repo.
    if matches!(classify_source_url(to), Ok(AllowedSource::GitHubReleases)) {
        return Ok(());
    }
    Err(reject(to))
}

pub fn sha512_hex(data: &[u8]) -> String {
    let digest = Sha512::digest(data);
    hex::encode(digest)
}

pub fn verify_sha512(data: &[u8], expected_hex: &str) -> Result<(), AppError> {
    let expected = expected_hex.trim().to_ascii_lowercase();
    if expected.len() != 128 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::HashMismatch {
            expected: expected.clone(),
            actual: String::new(),
            what: "invalid expected sha512".into(),
        });
    }
    let actual = sha512_hex(data);
    if actual != expected {
        return Err(AppError::HashMismatch {
            expected,
            actual,
            what: "file bytes".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_modrinth_and_project_releases() {
        assert!(is_allowed_source_url(
            "https://cdn.modrinth.com/data/AANobbMI/versions/sodium.jar"
        ));
        assert!(is_allowed_source_url(
            "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/mods-2026.11.2/arnold-snowfall-1.3.3.jar"
        ));
        assert!(is_allowed_source_url(
            "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/manifest.json"
        ));
        assert_eq!(
            classify_loader_url(
                "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json"
            )
            .unwrap(),
            AllowedSource::FabricMeta
        );
        assert!(!is_allowed_source_url(
            "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json"
        ));
    }

    #[test]
    fn rejects_other_hosts_and_lookalikes() {
        let bad = [
            "http://cdn.modrinth.com/data/x.jar",
            "https://cdn.modrinth.com.evil.com/data/x.jar",
            "https://evilcdn.modrinth.com/data/x.jar",
            "https://www.cdn.modrinth.com/data/x.jar",
            "https://cdn.modrinth.com@evil.com/data/x.jar",
            "https://evil.com/cdn.modrinth.com/data/x.jar",
            "https://github.com/evil/repo/releases/download/v1/a.jar",
            "https://github.com/Spottie97/other-repo/releases/download/v1/a.jar",
            "https://github.com/Spottie97/arnold-client-mod-installer/archive/refs/tags/v1.zip",
            "https://github.com/Spottie97/arnold-client-mod-installer/issues/1",
            "https://objects.githubusercontent.com/foo",
            "ftp://cdn.modrinth.com/x",
            "https://cdn.modrinth.com:8443/x.jar",
            "https://user:pass@cdn.modrinth.com/x.jar",
            "https://192.168.1.10/mod.jar",
            "https://cdn.modrinth.com/../escape.jar",
        ];
        for url in bad {
            assert!(classify_source_url(url).is_err(), "should reject {url}");
        }
    }

    #[test]
    fn redirect_only_from_allowlisted_origin_to_github_assets() {
        let origin = "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/mods-1/a.jar";
        assert!(is_allowed_redirect(
            origin,
            "https://objects.githubusercontent.com/github-production-release-asset/1"
        )
        .is_ok());
        assert!(
            is_allowed_redirect(origin, "https://release-assets.githubusercontent.com/a").is_ok()
        );
        assert!(is_allowed_redirect(origin, "https://evil.com/a").is_err());
        let latest = "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/manifest.json";
        assert!(is_allowed_redirect(
            latest,
            "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/mods-2026.11.2/manifest.json"
        )
        .is_ok());
        assert!(is_allowed_redirect(
            latest,
            "https://github.com/someone-else/arnold-client-mod-installer/releases/download/x/manifest.json"
        )
        .is_err());
        assert!(
            is_allowed_redirect(latest, "https://github.com/Spottie97/other-repo/releases/x")
                .is_err()
        );
        assert!(is_allowed_redirect(origin, "https://cdn.modrinth.com/data/x.jar").is_err());
        assert!(is_allowed_redirect(
            "https://evil.com/a.jar",
            "https://objects.githubusercontent.com/a"
        )
        .is_err());
    }

    #[test]
    fn sha512_accepts_match_and_rejects_mismatch() {
        let data = b"arnold-test-bytes";
        let good = sha512_hex(data);
        assert_eq!(good.len(), 128);
        verify_sha512(data, &good).unwrap();
        verify_sha512(data, &good.to_uppercase()).unwrap();
        let err = verify_sha512(b"other", &good).unwrap_err();
        assert_eq!(err.code(), "E-HASH-MISMATCH");
    }
}
