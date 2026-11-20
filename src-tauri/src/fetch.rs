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

    /// `on_bytes(done, total)` returns false to abort ([`AppError::Cancelled`]).
    fn fetch_with_progress(
        &self,
        url: &str,
        kind: FetchKind,
        on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, AppError> {
        let bytes = self.fetch_kind(url, kind)?;
        if !on_bytes(bytes.len() as u64, Some(bytes.len() as u64)) {
            return Err(AppError::Cancelled);
        }
        Ok(bytes)
    }

    fn local_root(&self) -> Option<&Path> {
        None
    }
}

pub(crate) const MAX_BODY_BYTES: u64 = 512 * 1024 * 1024;
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Read `reader` in 64 KiB chunks, stopping after `limit` bytes.
/// `on_bytes(done, total)` returns false to abort with [`AppError::Cancelled`].
pub(crate) fn read_body<R: Read>(
    reader: R,
    limit: u64,
    total: Option<u64>,
    on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<Vec<u8>, AppError> {
    let mut reader = reader.take(limit);
    let mut body = Vec::new();
    let mut buf = [0_u8; READ_CHUNK_BYTES];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
        if !on_bytes(body.len() as u64, total) {
            return Err(AppError::Cancelled);
        }
    }
    Ok(body)
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

    fn request(
        &self,
        start_url: &str,
        kind: FetchKind,
        on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, AppError> {
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
            let total = response
                .header("content-length")
                .and_then(|value| value.trim().parse::<u64>().ok());
            return read_body(response.into_reader(), MAX_BODY_BYTES, total, on_bytes);
        }
        Err(AppError::Io(format!("too many redirects from {start_url}")))
    }
}

impl Fetcher for UreqFetcher {
    fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
        self.request(url, kind, &mut |_, _| true)
    }

    fn fetch_with_progress(
        &self,
        url: &str,
        kind: FetchKind,
        on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, AppError> {
        self.request(url, kind, on_bytes)
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
    use std::io::Cursor;

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

    #[test]
    fn read_body_reports_rising_progress_and_returns_all_bytes() {
        let data = vec![9_u8; 300 * 1024];
        let mut seen = Vec::new();
        let got = read_body(
            Cursor::new(data.clone()),
            MAX_BODY_BYTES,
            Some(data.len() as u64),
            &mut |done, total| {
                seen.push((done, total));
                true
            },
        )
        .unwrap();
        assert_eq!(got, data);
        assert!(
            seen.len() > 1,
            "expected several chunks, got {}",
            seen.len()
        );
        let mut previous = 0_u64;
        for (done, total) in &seen {
            assert!(*done > previous);
            assert_eq!(*total, Some(data.len() as u64));
            previous = *done;
        }
        assert_eq!(seen.last().unwrap().0, data.len() as u64);
    }

    #[test]
    fn read_body_cancel_returns_cancelled() {
        let err = read_body(
            Cursor::new(vec![1_u8; 200 * 1024]),
            MAX_BODY_BYTES,
            None,
            &mut |_, _| false,
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-CANCELLED");
    }

    #[test]
    fn read_body_stops_at_the_limit() {
        let data = vec![7_u8; 1000];
        let mut seen = Vec::new();
        let got = read_body(
            Cursor::new(data.clone()),
            100,
            Some(1000),
            &mut |done, _| {
                seen.push(done);
                true
            },
        )
        .unwrap();
        assert_eq!(got, data[..100]);
        assert!(seen.iter().all(|done| *done <= 100));
        assert_eq!(*seen.last().unwrap(), 100);
    }
}
