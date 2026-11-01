use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::download::{classify_loader_url, classify_source_url, is_allowed_redirect};
use crate::errors::AppError;

/// Fetch bytes for an already-classified HTTPS URL.
pub trait Fetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, AppError> {
        self.fetch_kind(url, FetchKind::Component)
    }

    fn fetch_loader(&self, url: &str) -> Result<Vec<u8>, AppError> {
        self.fetch_kind(url, FetchKind::Loader)
    }

    fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError>;

    fn local_root(&self) -> Option<&Path> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    Component,
    Loader,
}

fn classify(url: &str, kind: FetchKind) -> Result<(), AppError> {
    match kind {
        FetchKind::Component => classify_source_url(url).map(|_| ()),
        FetchKind::Loader => classify_loader_url(url).map(|_| ()),
    }
}

/// In-memory fetcher for tests. Still enforces the allowlist.
#[derive(Debug, Default, Clone)]
pub struct MapFetcher {
    pub files: HashMap<String, Vec<u8>>,
    pub fail: HashSet<String>,
}

impl MapFetcher {
    pub fn insert(&mut self, url: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.files.insert(url.into(), bytes.into());
    }
}

impl Fetcher for MapFetcher {
    fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
        classify(url, kind)?;
        if self.fail.contains(url) {
            return Err(AppError::Io(format!("forced fetch failure: {url}")));
        }
        self.files
            .get(url)
            .cloned()
            .ok_or_else(|| AppError::Io(format!("not in map: {url}")))
    }
}

/// Production HTTPS fetcher. Redirects are followed only when `is_allowed_redirect` agrees.
pub struct UreqFetcher {
    agent: ureq::Agent,
}

impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqFetcher {
    pub fn new() -> Self {
        Self {
            agent: ureq::builder()
                .redirects(0)
                .timeout(Duration::from_secs(60))
                .build(),
        }
    }

    fn request(&self, start_url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
        classify(start_url, kind)?;
        let mut current = start_url.to_string();
        for _ in 0..5 {
            let response = self.agent.get(&current).call().map_err(|err| match err {
                ureq::Error::Status(code, _) => AppError::Io(format!("HTTP {code} for {current}")),
                other => AppError::Io(other.to_string()),
            })?;
            let status = response.status();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .header("location")
                    .ok_or_else(|| {
                        AppError::Io(format!("redirect without Location from {current}"))
                    })?
                    .to_string();
                let next = resolve_redirect(&current, &location)?;
                is_allowed_redirect(&current, &next)?;
                current = next;
                continue;
            }
            if !(200..300).contains(&status) {
                return Err(AppError::Io(format!("HTTP {status} for {current}")));
            }
            let mut body = Vec::new();
            response
                .into_reader()
                .take(512 * 1024 * 1024)
                .read_to_end(&mut body)?;
            return Ok(body);
        }
        Err(AppError::Io(format!("too many redirects from {start_url}")))
    }
}

impl Fetcher for UreqFetcher {
    fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
        self.request(url, kind)
    }
}

fn resolve_redirect(from: &str, location: &str) -> Result<String, AppError> {
    if location.starts_with("https://") {
        return Ok(location.to_string());
    }
    let base = url::Url::parse(from).map_err(|_| AppError::UrlNotAllowed {
        url: location.to_string(),
    })?;
    Ok(base
        .join(location)
        .map_err(|_| AppError::UrlNotAllowed {
            url: location.to_string(),
        })?
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_fetcher_enforces_allowlist() {
        let mut map = MapFetcher::default();
        map.insert("https://evil.example/a.jar", b"nope".to_vec());
        let err = map.fetch("https://evil.example/a.jar").unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }

    #[test]
    fn map_fetcher_serves_modrinth() {
        let mut map = MapFetcher::default();
        let url = "https://cdn.modrinth.com/data/AANobbMI/versions/sodium.jar";
        map.insert(url, b"sodium".to_vec());
        assert_eq!(map.fetch(url).unwrap(), b"sodium");
    }

    #[test]
    fn map_fetcher_allows_fabric_meta_as_loader_only() {
        let mut map = MapFetcher::default();
        let url = "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json";
        map.insert(url, b"{\"id\":\"fabric\"}".to_vec());
        let err = map.fetch(url).unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
        assert_eq!(map.fetch_loader(url).unwrap(), b"{\"id\":\"fabric\"}");
    }

    #[test]
    fn ureq_fetcher_rejects_before_network() {
        let err = UreqFetcher::new()
            .fetch("https://evil.example/x.jar")
            .unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }
}
