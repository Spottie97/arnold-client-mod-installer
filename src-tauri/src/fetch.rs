use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
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
        self.fetch_with_status(url, kind, &mut |done, total, _| on_bytes(done, total))
    }

    /// Same as [`Fetcher::fetch_with_progress`], plus a short retry/resume note.
    /// The note is empty while the transfer is moving normally.
    fn fetch_with_status(
        &self,
        url: &str,
        kind: FetchKind,
        on_bytes: &mut dyn FnMut(u64, Option<u64>, &str) -> bool,
    ) -> Result<Vec<u8>, AppError> {
        let bytes = self.fetch_kind(url, kind)?;
        if !on_bytes(bytes.len() as u64, Some(bytes.len() as u64), "") {
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
const BACKOFF_SLICE: Duration = Duration::from_millis(100);

/// Connect, per-read idle, and per-write limits. There is no overall deadline:
/// ureq's `.timeout()` would override `timeout_read` and cap the whole body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NetPolicy {
    pub connect: Duration,
    pub read_idle: Duration,
    pub write: Duration,
}

pub(crate) const DEFAULT_NET: NetPolicy = NetPolicy {
    connect: Duration::from_secs(20),
    read_idle: Duration::from_secs(30),
    write: Duration::from_secs(30),
};

pub(crate) fn build_agent(policy: &NetPolicy) -> ureq::Agent {
    ureq::builder()
        .redirects(0)
        .timeout_connect(policy.connect)
        .timeout_read(policy.read_idle)
        .timeout_write(policy.write)
        .build()
}

#[derive(Debug)]
pub(crate) enum ReadFail {
    Cancelled,
    Io(io::Error),
}

/// Append `reader` onto `body` in 64 KiB chunks, stopping at `limit` total bytes.
/// `on_bytes` sees the cumulative length. `false` aborts with [`ReadFail::Cancelled`].
pub(crate) fn read_body_into<R: Read>(
    reader: R,
    limit: u64,
    body: &mut Vec<u8>,
    total: Option<u64>,
    on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<(), ReadFail> {
    let room = limit.saturating_sub(body.len() as u64);
    let mut reader = reader.take(room);
    let mut buf = [0_u8; READ_CHUNK_BYTES];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) => return Err(ReadFail::Io(err)),
        };
        body.extend_from_slice(&buf[..n]);
        if !on_bytes(body.len() as u64, total) {
            return Err(ReadFail::Cancelled);
        }
    }
    Ok(())
}

/// Read `reader` in 64 KiB chunks, stopping after `limit` bytes.
/// `on_bytes(done, total)` returns false to abort with [`AppError::Cancelled`].
pub(crate) fn read_body<R: Read>(
    reader: R,
    limit: u64,
    total: Option<u64>,
    on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<Vec<u8>, AppError> {
    let mut body = Vec::new();
    match read_body_into(reader, limit, &mut body, total, on_bytes) {
        Ok(()) => Ok(body),
        Err(ReadFail::Cancelled) => Err(AppError::Cancelled),
        Err(ReadFail::Io(err)) => Err(AppError::Io(err.to_string())),
    }
}

#[derive(Debug)]
pub(crate) struct Opened<R> {
    pub reader: R,
    pub resumed: bool,
    pub total: Option<u64>,
    pub validator: Option<String>,
}

#[derive(Debug)]
pub(crate) struct RetryableFail {
    pub message: String,
    pub restart: bool,
}

#[derive(Debug)]
pub(crate) enum AttemptErr {
    Retryable(RetryableFail),
    Fatal(AppError),
}

impl AttemptErr {
    fn retry(message: impl Into<String>) -> Self {
        Self::Retryable(RetryableFail {
            message: message.into(),
            restart: false,
        })
    }

    fn restart(message: impl Into<String>) -> Self {
        Self::Retryable(RetryableFail {
            message: message.into(),
            restart: true,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    pub max_attempts: u32,
    pub hard_cap: u32,
    pub progress_reset_bytes: u64,
    pub backoff: &'static [Duration],
}

pub(crate) const DEFAULT_RETRY: RetryPolicy = RetryPolicy {
    max_attempts: 5,
    hard_cap: 12,
    progress_reset_bytes: 8 << 20,
    backoff: &[
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(4),
        Duration::from_secs(8),
    ],
};

const NO_RETRY: RetryPolicy = RetryPolicy {
    max_attempts: 1,
    hard_cap: 1,
    progress_reset_bytes: u64::MAX,
    backoff: &[],
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentRange {
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub total: Option<u64>,
}

pub(crate) fn parse_content_range(value: &str) -> Option<ContentRange> {
    let value = value.trim();
    if value.len() < 6 || !value[..5].eq_ignore_ascii_case("bytes") || value.as_bytes()[5] != b' ' {
        return None;
    }
    let rest = value[6..].trim();
    if let Some(total) = rest.strip_prefix("*/") {
        return Some(ContentRange {
            start: None,
            end: None,
            total: parse_range_total(total.trim())?,
        });
    }
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.trim().split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let end = end.trim().parse::<u64>().ok()?;
    if end < start {
        return None;
    }
    Some(ContentRange {
        start: Some(start),
        end: Some(end),
        total: parse_range_total(total.trim())?,
    })
}

fn parse_range_total(value: &str) -> Option<Option<u64>> {
    if value == "*" {
        Some(None)
    } else {
        value.parse::<u64>().ok().map(Some)
    }
}

fn strong_validator(etag: Option<&str>, last_modified: Option<&str>) -> Option<String> {
    if let Some(etag) = etag.map(str::trim).filter(|value| !value.is_empty()) {
        let weak = etag.len() >= 2 && etag[..2].eq_ignore_ascii_case("W/");
        if !weak {
            return Some(etag.to_string());
        }
    }
    last_modified
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

struct ResponseView<'a> {
    status: u16,
    content_range: Option<&'a str>,
    content_length: Option<&'a str>,
    etag: Option<&'a str>,
    last_modified: Option<&'a str>,
}

fn opened_from_parts<R>(
    view: ResponseView<'_>,
    offset: u64,
    reader: R,
    url: &str,
) -> Result<Opened<R>, AttemptErr> {
    if view.status == 206 {
        let Some(range) = view.content_range.and_then(parse_content_range) else {
            return Err(AttemptErr::restart(format!(
                "unparsable Content-Range for {url}"
            )));
        };
        if range.start != Some(offset) {
            return Err(AttemptErr::restart(format!(
                "Content-Range start {start:?} != {offset} for {url}",
                start = range.start
            )));
        }
        return Ok(Opened {
            reader,
            resumed: true,
            total: range.total,
            validator: strong_validator(view.etag, view.last_modified),
        });
    }
    if !(200..300).contains(&view.status) {
        return Err(AttemptErr::Fatal(AppError::Io(format!(
            "HTTP {} for {url}",
            view.status
        ))));
    }
    Ok(Opened {
        reader,
        resumed: false,
        total: view
            .content_length
            .and_then(|value| value.trim().parse().ok()),
        validator: strong_validator(view.etag, view.last_modified),
    })
}

/// Status line shown while a download is waiting to try again.
pub(crate) fn retry_note(
    body_len: u64,
    ignored_range: bool,
    from_start: bool,
    attempt: u32,
    policy: &RetryPolicy,
) -> String {
    let of = if attempt <= policy.max_attempts {
        policy.max_attempts
    } else {
        policy.hard_cap
    };
    let label = format!("attempt {attempt} of {of}");
    if ignored_range {
        format!("retrying from the start (server ignored Range) — {label}")
    } else if body_len > 0 {
        let mb = body_len as f64 / (1024.0 * 1024.0);
        format!("retrying, resuming at {mb:.0} MB — {label}")
    } else if from_start {
        format!("retrying from the start — {label}")
    } else {
        format!("retrying — {label}")
    }
}

struct Notice {
    done: u64,
    total: Option<u64>,
    ignored_range: bool,
    from_start: bool,
    next_attempt: u32,
    delay: Duration,
}

fn note_and_wait(
    notice: Notice,
    policy: &RetryPolicy,
    sleep: &dyn Fn(Duration),
    on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    on_note: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    on_note(&retry_note(
        notice.done,
        notice.ignored_range && notice.done == 0,
        notice.from_start,
        notice.next_attempt,
        policy,
    ));
    wait_cancellable(notice.delay, sleep, on_bytes, notice.done, notice.total)
}

fn wait_cancellable(
    delay: Duration,
    sleep: &dyn Fn(Duration),
    on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    done: u64,
    total: Option<u64>,
) -> Result<(), AppError> {
    let mut left = delay;
    while !left.is_zero() {
        if !on_bytes(done, total) {
            return Err(AppError::Cancelled);
        }
        let step = BACKOFF_SLICE.min(left);
        sleep(step);
        left = left.saturating_sub(step);
    }
    Ok(())
}

fn gave_up(last_err: &str, attempts: u32, done: u64, total: Option<u64>) -> AppError {
    let done_mb = done as f64 / (1024.0 * 1024.0);
    let total_mb = total.unwrap_or(0) as f64 / (1024.0 * 1024.0);
    AppError::Io(format!(
        "{last_err} — gave up after {attempts} attempts ({done_mb:.1}/{total_mb:.1} MB received)"
    ))
}

fn backoff_delay(policy: &RetryPolicy, attempt_no: u32) -> Duration {
    let index = attempt_no.saturating_sub(1) as usize;
    policy
        .backoff
        .get(index)
        .or_else(|| policy.backoff.last())
        .copied()
        .unwrap_or(Duration::ZERO)
}

fn is_retryable_io(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::Interrupted
    )
}

type OpenAttempt<'a, R> = &'a mut dyn FnMut(u64, Option<&str>) -> Result<Opened<R>, AttemptErr>;

pub(crate) fn download_resumable<R: Read>(
    open: OpenAttempt<'_, R>,
    limit: u64,
    policy: &RetryPolicy,
    sleep: &dyn Fn(Duration),
    on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    on_note: &mut dyn FnMut(&str),
) -> Result<Vec<u8>, AppError> {
    let mut body = Vec::new();
    let mut validator: Option<String> = None;
    let mut known_total: Option<u64> = None;
    let mut counters = Counters {
        attempt_no: 0,
        streak: 0,
    };
    let mut from_start = false;

    loop {
        if counters.attempt_no > 0 {
            on_note(&retry_note(
                body.len() as u64,
                false,
                from_start,
                counters.attempt_no + 1,
                policy,
            ));
        }
        let offset = body.len() as u64;
        let opened = match open(offset, validator.as_deref()) {
            Ok(opened) => opened,
            Err(AttemptErr::Fatal(err)) => return Err(err),
            Err(AttemptErr::Retryable(fail)) => {
                if fail.restart {
                    body.clear();
                    validator = None;
                    from_start = true;
                }
                let mut hooks = ProgressHooks {
                    sleep,
                    on_bytes: &mut *on_bytes,
                    on_note: &mut *on_note,
                };
                account_failure(
                    &mut counters,
                    FailContext {
                        message: &fail.message,
                        added: 0,
                        done: body.len() as u64,
                        total: known_total,
                        ignored_range: false,
                        from_start,
                    },
                    policy,
                    &mut hooks,
                )?;
                continue;
            }
        };

        if let Some(next) = opened.validator.clone() {
            validator = Some(next);
        }
        let mut ignored_range = false;
        if offset > 0 && !opened.resumed {
            body.clear();
            ignored_range = true;
            from_start = true;
        } else if opened.resumed {
            from_start = false;
        }
        if let Some(total) = opened.total {
            known_total = Some(total);
        }
        let total = opened.total.or(known_total);
        if ignored_range {
            on_note(&retry_note(0, true, true, counters.attempt_no + 1, policy));
        }

        let before = body.len() as u64;
        let read = read_body_into(opened.reader, limit, &mut body, total, on_bytes);
        let failure = match read {
            Err(ReadFail::Cancelled) => return Err(AppError::Cancelled),
            Err(ReadFail::Io(err)) if !is_retryable_io(&err) => {
                return Err(AppError::Io(err.to_string()));
            }
            Err(ReadFail::Io(err)) => Some(err.to_string()),
            Ok(()) => {
                let at_cap = body.len() as u64 >= limit;
                let short = !at_cap && total.is_some_and(|expected| (body.len() as u64) < expected);
                if short {
                    Some("connection closed before the download finished".to_string())
                } else {
                    return Ok(body);
                }
            }
        };
        let added = (body.len() as u64).saturating_sub(before);
        let message = failure.expect("retryable read failure");
        let mut hooks = ProgressHooks {
            sleep,
            on_bytes: &mut *on_bytes,
            on_note: &mut *on_note,
        };
        account_failure(
            &mut counters,
            FailContext {
                message: &message,
                added,
                done: body.len() as u64,
                total: known_total,
                ignored_range,
                from_start,
            },
            policy,
            &mut hooks,
        )?;
    }
}

struct Counters {
    attempt_no: u32,
    streak: u32,
}

struct FailContext<'a> {
    message: &'a str,
    added: u64,
    done: u64,
    total: Option<u64>,
    ignored_range: bool,
    from_start: bool,
}

struct ProgressHooks<'a> {
    sleep: &'a dyn Fn(Duration),
    on_bytes: &'a mut dyn FnMut(u64, Option<u64>) -> bool,
    on_note: &'a mut dyn FnMut(&str),
}

fn account_failure(
    counters: &mut Counters,
    ctx: FailContext<'_>,
    policy: &RetryPolicy,
    hooks: &mut ProgressHooks<'_>,
) -> Result<(), AppError> {
    counters.attempt_no += 1;
    if ctx.added >= policy.progress_reset_bytes {
        counters.streak = 0;
    } else {
        counters.streak += 1;
    }
    if counters.streak >= policy.max_attempts || counters.attempt_no >= policy.hard_cap {
        return Err(gave_up(
            ctx.message,
            counters.attempt_no,
            ctx.done,
            ctx.total,
        ));
    }
    note_and_wait(
        Notice {
            done: ctx.done,
            total: ctx.total,
            ignored_range: ctx.ignored_range,
            from_start: ctx.from_start,
            next_attempt: counters.attempt_no + 1,
            delay: backoff_delay(policy, counters.attempt_no),
        },
        policy,
        hooks.sleep,
        &mut *hooks.on_bytes,
        &mut *hooks.on_note,
    )
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

type BodyReader = Box<dyn Read + Send + Sync>;

/// Production HTTPS fetcher. Redirects are followed only when `is_allowed_redirect` agrees.
pub struct UreqFetcher {
    agent: ureq::Agent,
    retry: RetryPolicy,
}

impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqFetcher {
    pub fn new() -> Self {
        Self::with_policy(DEFAULT_NET, DEFAULT_RETRY)
    }

    pub(crate) fn with_policy(net: NetPolicy, retry: RetryPolicy) -> Self {
        Self {
            agent: build_agent(&net),
            retry,
        }
    }

    fn fetch_resumable(
        &self,
        url: &str,
        kind: FetchKind,
        policy: &RetryPolicy,
        on_bytes: &mut dyn FnMut(u64, Option<u64>, &str) -> bool,
    ) -> Result<Vec<u8>, AppError> {
        let note = std::cell::RefCell::new(String::new());
        download_resumable(
            &mut |offset, validator| self.open_attempt(url, kind, offset, validator),
            MAX_BODY_BYTES,
            policy,
            &|delay| std::thread::sleep(delay),
            &mut |done, total| {
                let current = note.borrow().clone();
                on_bytes(done, total, &current)
            },
            &mut |text| {
                *note.borrow_mut() = text.to_string();
            },
        )
    }

    fn open_attempt(
        &self,
        start_url: &str,
        kind: FetchKind,
        offset: u64,
        validator: Option<&str>,
    ) -> Result<Opened<BodyReader>, AttemptErr> {
        classify(start_url, kind).map_err(AttemptErr::Fatal)?;
        let mut current = start_url.to_string();
        for _ in 0..5 {
            match self.one_hop(&current, offset, validator)? {
                Hop::Redirect(location) => {
                    let next = resolve_redirect(&current, &location).map_err(AttemptErr::Fatal)?;
                    is_allowed_redirect(&current, &next).map_err(AttemptErr::Fatal)?;
                    current = next;
                }
                Hop::Ready(opened) => return Ok(opened),
            }
        }
        Err(AttemptErr::Fatal(AppError::Io(format!(
            "too many redirects from {start_url}"
        ))))
    }

    fn one_hop(&self, url: &str, offset: u64, validator: Option<&str>) -> Result<Hop, AttemptErr> {
        let mut request = self.agent.get(url);
        if offset > 0 {
            request = request.set("Range", &format!("bytes={offset}-"));
            if let Some(validator) = validator {
                request = request.set("If-Range", validator);
            }
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) => {
                return self.status_error(status, response, url);
            }
            Err(ureq::Error::Transport(err)) => return Err(map_transport(err)),
        };
        self.success_or_redirect(response, offset, url)
    }

    fn status_error(
        &self,
        status: u16,
        response: ureq::Response,
        url: &str,
    ) -> Result<Hop, AttemptErr> {
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = response.header("location").map(str::to_string);
            return redirect_hop(location, url);
        }
        drop(response);
        Err(http_status(status, url))
    }

    fn success_or_redirect(
        &self,
        response: ureq::Response,
        offset: u64,
        url: &str,
    ) -> Result<Hop, AttemptErr> {
        let status = response.status();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = response.header("location").map(str::to_string);
            return redirect_hop(location, url);
        }
        if status == 416 {
            return Err(AttemptErr::restart(format!("HTTP 416 for {url}")));
        }
        if matches!(status, 408 | 429 | 500 | 502 | 503 | 504) {
            return Err(AttemptErr::retry(format!("HTTP {status} for {url}")));
        }
        let content_range = response.header("content-range").map(str::to_string);
        let content_length = response.header("content-length").map(str::to_string);
        let etag = response.header("etag").map(str::to_string);
        let last_modified = response.header("last-modified").map(str::to_string);
        let reader = response.into_reader();
        opened_from_parts(
            ResponseView {
                status,
                content_range: content_range.as_deref(),
                content_length: content_length.as_deref(),
                etag: etag.as_deref(),
                last_modified: last_modified.as_deref(),
            },
            offset,
            reader,
            url,
        )
        .map(Hop::Ready)
    }
}

enum Hop {
    Redirect(String),
    Ready(Opened<BodyReader>),
}

fn redirect_hop(location: Option<String>, url: &str) -> Result<Hop, AttemptErr> {
    match location {
        Some(location) => Ok(Hop::Redirect(location)),
        None => Err(AttemptErr::Fatal(AppError::Io(format!(
            "redirect without Location from {url}"
        )))),
    }
}

fn http_status(status: u16, url: &str) -> AttemptErr {
    if status == 416 {
        AttemptErr::restart(format!("HTTP 416 for {url}"))
    } else if matches!(status, 408 | 429 | 500 | 502 | 503 | 504) {
        AttemptErr::retry(format!("HTTP {status} for {url}"))
    } else {
        AttemptErr::Fatal(AppError::Io(format!("HTTP {status} for {url}")))
    }
}

fn map_transport(err: ureq::Transport) -> AttemptErr {
    let retryable = matches!(
        err.kind(),
        ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io
    );
    let message = err.to_string();
    if retryable {
        AttemptErr::retry(message)
    } else {
        AttemptErr::Fatal(AppError::Io(message))
    }
}

impl Fetcher for UreqFetcher {
    fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
        self.fetch_resumable(url, kind, &NO_RETRY, &mut |_, _, _| true)
    }

    fn fetch_with_progress(
        &self,
        url: &str,
        kind: FetchKind,
        on_bytes: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, AppError> {
        self.fetch_resumable(url, kind, &self.retry, &mut |done, total, _| {
            on_bytes(done, total)
        })
    }

    fn fetch_with_status(
        &self,
        url: &str,
        kind: FetchKind,
        on_bytes: &mut dyn FnMut(u64, Option<u64>, &str) -> bool,
    ) -> Result<Vec<u8>, AppError> {
        self.fetch_resumable(url, kind, &self.retry, on_bytes)
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
    use crate::download::{sha512_hex, verify_sha512};
    use std::cell::RefCell;
    use std::io::{Cursor, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

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

    #[test]
    fn read_body_into_appends_and_respects_cap_across_calls() {
        let limit = 1024 * 1024;
        let mut body = Vec::new();
        let first = vec![1_u8; 600 * 1024];
        read_body_into(
            Cursor::new(first.clone()),
            limit,
            &mut body,
            None,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(body, first);
        let second = vec![2_u8; 600 * 1024];
        read_body_into(
            Cursor::new(second),
            limit,
            &mut body,
            Some(limit),
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(body.len(), limit as usize);
        assert!(body[..600 * 1024].iter().all(|byte| *byte == 1));
        assert!(body[600 * 1024..].iter().all(|byte| *byte == 2));
    }

    #[test]
    fn retry_notes_name_resume_restart_and_attempt() {
        let resumed = retry_note(86 * 1024 * 1024, false, false, 2, &DEFAULT_RETRY);
        assert_eq!(resumed, "retrying, resuming at 86 MB — attempt 2 of 5");
        let ignored = retry_note(0, true, true, 2, &DEFAULT_RETRY);
        assert_eq!(
            ignored,
            "retrying from the start (server ignored Range) — attempt 2 of 5"
        );
    }

    struct PrefixThen {
        data: Vec<u8>,
        pos: usize,
        kind: io::ErrorKind,
        message: &'static str,
    }

    impl Read for PrefixThen {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos < self.data.len() {
                let n = (self.data.len() - self.pos).min(buf.len());
                buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            Err(io::Error::new(self.kind, self.message))
        }
    }

    fn prefix_then(data: &[u8], kind: io::ErrorKind) -> PrefixThen {
        PrefixThen {
            data: data.to_vec(),
            pos: 0,
            kind,
            message: "timed out reading response",
        }
    }

    #[test]
    fn resume_after_mid_body_timeout_uses_range_and_matches_bytes() {
        let payload = vec![7_u8; 600 * 1024];
        let cut = 300 * 1024;
        let calls = RefCell::new(Vec::new());
        let mut seen = Vec::new();
        let sleeps = RefCell::new(Vec::new());
        let notes = RefCell::new(Vec::new());
        let got = download_resumable(
            &mut |offset, validator| {
                calls
                    .borrow_mut()
                    .push((offset, validator.map(str::to_string)));
                if offset == 0 {
                    Ok(Opened {
                        reader: Box::new(prefix_then(&payload[..cut], io::ErrorKind::TimedOut))
                            as Box<dyn Read>,
                        resumed: false,
                        total: Some(payload.len() as u64),
                        validator: Some("\"etag\"".into()),
                    })
                } else {
                    Ok(Opened {
                        reader: Box::new(Cursor::new(payload[offset as usize..].to_vec()))
                            as Box<dyn Read>,
                        resumed: true,
                        total: Some(payload.len() as u64),
                        validator: Some("\"etag\"".into()),
                    })
                }
            },
            MAX_BODY_BYTES,
            &DEFAULT_RETRY,
            &|delay| sleeps.borrow_mut().push(delay),
            &mut |done, _| {
                seen.push(done);
                true
            },
            &mut |note| notes.borrow_mut().push(note.to_string()),
        )
        .unwrap();
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[1].0, cut as u64);
        assert_eq!(calls[1].1.as_deref(), Some("\"etag\""));
        assert_eq!(got, payload);
        verify_sha512(&got, &sha512_hex(&payload)).unwrap();
        let mut previous = 0_u64;
        for done in &seen {
            assert!(*done >= previous, "progress went from {previous} to {done}");
            previous = *done;
        }
        let slept: Duration = sleeps.into_inner().into_iter().sum();
        assert_eq!(slept, Duration::from_secs(1));
        assert!(
            notes
                .into_inner()
                .iter()
                .any(|note| note.contains("resuming at") && note.contains("attempt 2 of 5")),
            "expected a resume note"
        );
    }

    #[test]
    fn range_ignored_restarts_from_zero_without_duplication() {
        let payload = vec![4_u8; 80 * 1024];
        let calls = RefCell::new(0_u32);
        let got = download_resumable(
            &mut |offset, _| {
                *calls.borrow_mut() += 1;
                if offset == 0 && *calls.borrow() == 1 {
                    Ok(Opened {
                        reader: Box::new(prefix_then(
                            &payload[..40 * 1024],
                            io::ErrorKind::TimedOut,
                        )) as Box<dyn Read>,
                        resumed: false,
                        total: Some(payload.len() as u64),
                        validator: Some("\"v\"".into()),
                    })
                } else {
                    assert!(offset > 0);
                    Ok(Opened {
                        reader: Box::new(Cursor::new(payload.clone())) as Box<dyn Read>,
                        resumed: false,
                        total: Some(payload.len() as u64),
                        validator: Some("\"v\"".into()),
                    })
                }
            },
            MAX_BODY_BYTES,
            &DEFAULT_RETRY,
            &|_| {},
            &mut |_, _| true,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(got, payload);
        assert_eq!(got.len(), payload.len());
        verify_sha512(&got, &sha512_hex(&payload)).unwrap();
    }

    #[test]
    fn mismatched_content_range_start_restarts() {
        let err = opened_from_parts(
            ResponseView {
                status: 206,
                content_range: Some("bytes 0-9/100"),
                content_length: None,
                etag: Some("\"etag\""),
                last_modified: None,
            },
            100,
            Cursor::new(Vec::<u8>::new()),
            "https://cdn.modrinth.com/x",
        )
        .unwrap_err();
        match err {
            AttemptErr::Retryable(fail) => assert!(fail.restart),
            other => panic!("expected restart, got {other:?}"),
        }

        let payload = b"abcdefghijklmnopqrstuvwxyz".to_vec();
        let calls = RefCell::new(Vec::new());
        let got = download_resumable(
            &mut |offset, validator| {
                calls
                    .borrow_mut()
                    .push((offset, validator.map(str::to_string)));
                let n = calls.borrow().len();
                if n == 1 {
                    Ok(Opened {
                        reader: Box::new(prefix_then(&payload[..4], io::ErrorKind::TimedOut))
                            as Box<dyn Read>,
                        resumed: false,
                        total: Some(payload.len() as u64),
                        validator: Some("\"etag\"".into()),
                    })
                } else if n == 2 {
                    Err(AttemptErr::restart("mismatched Content-Range"))
                } else {
                    Ok(Opened {
                        reader: Box::new(Cursor::new(payload.clone())) as Box<dyn Read>,
                        resumed: false,
                        total: Some(payload.len() as u64),
                        validator: None,
                    })
                }
            },
            MAX_BODY_BYTES,
            &DEFAULT_RETRY,
            &|_| {},
            &mut |_, _| true,
            &mut |_| {},
        )
        .unwrap();
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(calls[1].0, 4);
        assert_eq!(calls[1].1.as_deref(), Some("\"etag\""));
        assert_eq!(calls[2].0, 0);
        assert_eq!(calls[2].1, None);
        assert_eq!(got, payload);
    }

    #[test]
    fn gives_up_after_max_attempts_without_progress() {
        let calls = RefCell::new(0_u32);
        let sleeps = RefCell::new(Vec::new());
        let err = download_resumable(
            &mut |_, _| {
                *calls.borrow_mut() += 1;
                Ok(Opened {
                    reader: Box::new(prefix_then(&[], io::ErrorKind::TimedOut)) as Box<dyn Read>,
                    resumed: false,
                    total: Some(1024),
                    validator: None,
                })
            },
            MAX_BODY_BYTES,
            &DEFAULT_RETRY,
            &|delay| sleeps.borrow_mut().push(delay),
            &mut |_, _| true,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(*calls.borrow(), 5);
        assert_eq!(err.code(), "E-IO");
        assert!(err.to_string().contains("after 5 attempts"), "{err}");
        let slices = sleeps.into_inner();
        assert_eq!(
            slices.iter().copied().sum::<Duration>(),
            Duration::from_secs(15)
        );
        let mut offset = 0;
        for seconds in [1_u64, 2, 4, 8] {
            let count = (seconds * 10) as usize;
            let group: Duration = slices[offset..offset + count].iter().copied().sum();
            assert_eq!(group, Duration::from_secs(seconds));
            offset += count;
        }
        assert_eq!(offset, slices.len());
    }

    #[test]
    fn progress_resets_attempt_budget_but_hard_cap_holds() {
        const FAST: RetryPolicy = RetryPolicy {
            max_attempts: 5,
            hard_cap: 12,
            progress_reset_bytes: 8,
            backoff: &[Duration::ZERO],
        };
        let calls = RefCell::new(0_u32);
        let err = download_resumable(
            &mut |_, _| {
                *calls.borrow_mut() += 1;
                Ok(Opened {
                    reader: Box::new(prefix_then(&[9_u8; 8], io::ErrorKind::TimedOut))
                        as Box<dyn Read>,
                    resumed: true,
                    total: Some(u64::MAX),
                    validator: None,
                })
            },
            MAX_BODY_BYTES,
            &FAST,
            &|_| {},
            &mut |_, _| true,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(*calls.borrow(), 12);
        assert_eq!(err.code(), "E-IO");
        assert!(err.to_string().contains("after 12 attempts"), "{err}");
    }

    #[test]
    fn fatal_status_is_not_retried() {
        for fatal in [
            AttemptErr::Fatal(AppError::UrlNotAllowed {
                url: "https://evil.example/x".into(),
            }),
            AttemptErr::Fatal(AppError::Io(
                "HTTP 404 for https://cdn.modrinth.com/missing.jar".into(),
            )),
        ] {
            let calls = RefCell::new(0_u32);
            let expected = match &fatal {
                AttemptErr::Fatal(err) => err.code(),
                AttemptErr::Retryable(_) => unreachable!(),
            };
            let err = download_resumable::<Cursor<Vec<u8>>>(
                &mut |_, _| {
                    *calls.borrow_mut() += 1;
                    Err(match &fatal {
                        AttemptErr::Fatal(AppError::UrlNotAllowed { url }) => {
                            AttemptErr::Fatal(AppError::UrlNotAllowed { url: url.clone() })
                        }
                        AttemptErr::Fatal(AppError::Io(message)) => {
                            AttemptErr::Fatal(AppError::Io(message.clone()))
                        }
                        _ => unreachable!(),
                    })
                },
                MAX_BODY_BYTES,
                &DEFAULT_RETRY,
                &|_| {},
                &mut |_, _| true,
                &mut |_| {},
            )
            .unwrap_err();
            assert_eq!(*calls.borrow(), 1);
            assert_eq!(err.code(), expected);
        }
    }

    #[test]
    fn cancel_mid_file_is_not_retried() {
        let calls = RefCell::new(0_u32);
        let mut chunks = 0_u32;
        let err = download_resumable(
            &mut |_, _| {
                *calls.borrow_mut() += 1;
                Ok(Opened {
                    reader: Box::new(Cursor::new(vec![1_u8; 200 * 1024])) as Box<dyn Read>,
                    resumed: false,
                    total: Some(200 * 1024),
                    validator: None,
                })
            },
            MAX_BODY_BYTES,
            &DEFAULT_RETRY,
            &|_| {},
            &mut |_, _| {
                chunks += 1;
                chunks < 2
            },
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-CANCELLED");
        assert_eq!(*calls.borrow(), 1);
        assert_eq!(chunks, 2);
    }

    #[test]
    fn cancel_during_backoff_returns_cancelled_without_reopening() {
        let calls = RefCell::new(0_u32);
        let err = download_resumable::<Cursor<Vec<u8>>>(
            &mut |_, _| {
                *calls.borrow_mut() += 1;
                Err(AttemptErr::retry("connection reset"))
            },
            MAX_BODY_BYTES,
            &DEFAULT_RETRY,
            &|_| {},
            &mut |_, _| false,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-CANCELLED");
        assert_eq!(*calls.borrow(), 1);
    }

    #[test]
    fn parse_content_range_accepts_a_span_a_star_and_rejects_garbage() {
        assert_eq!(
            parse_content_range("bytes 300-399/1000"),
            Some(ContentRange {
                start: Some(300),
                end: Some(399),
                total: Some(1000),
            })
        );
        assert_eq!(
            parse_content_range("bytes */1000"),
            Some(ContentRange {
                start: None,
                end: None,
                total: Some(1000),
            })
        );
        assert_eq!(
            parse_content_range("bytes 0-99/*"),
            Some(ContentRange {
                start: Some(0),
                end: Some(99),
                total: None,
            })
        );
        assert_eq!(parse_content_range("nope"), None);
        assert_eq!(parse_content_range("bytes abc"), None);
        assert_eq!(parse_content_range(""), None);
    }

    fn recv_headers(sock: &mut TcpStream) -> String {
        sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut buf = Vec::new();
        let mut byte = [0_u8; 1];
        while buf.len() < 16 * 1024 {
            match sock.read(&mut byte) {
                Ok(1) => {
                    buf.push(byte[0]);
                    if buf.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                _ => break,
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
        headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }

    #[test]
    fn slow_but_moving_body_outlives_idle_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let chunks = 14_usize;
        let chunk = vec![5_u8; 64 * 1024];
        let total = chunks * chunk.len();
        let server = thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_nodelay(true).ok();
            let _ = recv_headers(&mut sock);
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n");
            sock.write_all(head.as_bytes()).unwrap();
            for index in 0..chunks {
                if index > 0 {
                    thread::sleep(Duration::from_millis(150));
                }
                sock.write_all(&chunk).unwrap();
            }
        });
        // The pause stays far under the idle limit. A 300ms limit is close
        // enough that a busy macOS runner can delay the next chunk past it.
        let agent = build_agent(&NetPolicy {
            connect: Duration::from_secs(2),
            read_idle: Duration::from_secs(2),
            write: Duration::from_secs(2),
        });
        let response = agent
            .get(&format!("http://127.0.0.1:{port}/slow"))
            .call()
            .unwrap();
        let mut body = Vec::new();
        read_body_into(
            response.into_reader(),
            MAX_BODY_BYTES,
            &mut body,
            Some(total as u64),
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(body.len(), total);
        server.join().unwrap();
    }

    #[test]
    fn stalled_body_hits_idle_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_nodelay(true).ok();
            let _ = recv_headers(&mut sock);
            let total = 64 * 1024;
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n");
            sock.write_all(head.as_bytes()).unwrap();
            sock.write_all(&vec![1_u8; total / 2]).unwrap();
            thread::sleep(Duration::from_secs(1));
        });
        let agent = build_agent(&NetPolicy {
            connect: Duration::from_secs(2),
            read_idle: Duration::from_millis(300),
            write: Duration::from_secs(2),
        });
        let response = agent
            .get(&format!("http://127.0.0.1:{port}/stall"))
            .call()
            .unwrap();
        let mut body = Vec::new();
        let err = read_body_into(
            response.into_reader(),
            MAX_BODY_BYTES,
            &mut body,
            Some(64 * 1024),
            &mut |_, _| true,
        )
        .unwrap_err();
        match err {
            ReadFail::Io(err) => assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}"),
            ReadFail::Cancelled => panic!("stall was cancelled"),
        }
        server.join().unwrap();
    }

    #[test]
    fn local_server_range_resume_end_to_end() {
        let payload: Vec<u8> = (0..200_000).map(|n| (n % 251) as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = payload.clone();
        let server = thread::spawn(move || {
            for expected_range in [false, true] {
                let (mut sock, _) = listener.accept().unwrap();
                sock.set_nodelay(true).ok();
                let headers = recv_headers(&mut sock);
                let range = header_value(&headers, "range");
                if expected_range {
                    let range = range.expect("second request should resume");
                    assert!(range.starts_with("bytes="), "{range}");
                    assert!(
                        header_value(&headers, "if-range")
                            .is_some_and(|value| value.contains("v1")),
                        "{headers}"
                    );
                    let start: usize = range
                        .trim_start_matches("bytes=")
                        .trim_end_matches('-')
                        .parse()
                        .unwrap();
                    let rest = &body[start..];
                    let end = body.len() - 1;
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {len}\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                        total = body.len(),
                        len = rest.len()
                    );
                    sock.write_all(head.as_bytes()).unwrap();
                    sock.write_all(rest).unwrap();
                } else {
                    assert!(range.is_none(), "{headers}");
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                        len = body.len()
                    );
                    sock.write_all(head.as_bytes()).unwrap();
                    sock.write_all(&body[..body.len() / 3]).unwrap();
                }
            }
        });
        let agent = build_agent(&NetPolicy {
            connect: Duration::from_secs(2),
            read_idle: Duration::from_secs(2),
            write: Duration::from_secs(2),
        });
        let url = format!("http://127.0.0.1:{port}/file");
        const FAST: RetryPolicy = RetryPolicy {
            max_attempts: 5,
            hard_cap: 12,
            progress_reset_bytes: 8 << 20,
            backoff: &[Duration::ZERO],
        };
        let got = download_resumable(
            &mut |offset, validator| {
                let mut request = agent.get(&url);
                if offset > 0 {
                    request = request.set("Range", &format!("bytes={offset}-"));
                    if let Some(validator) = validator {
                        request = request.set("If-Range", validator);
                    }
                }
                let response = request.call().map_err(|err| match err {
                    ureq::Error::Status(code, _) => {
                        AttemptErr::Fatal(AppError::Io(format!("HTTP {code}")))
                    }
                    ureq::Error::Transport(err) => map_transport(err),
                })?;
                let status = response.status();
                let content_range = response.header("content-range").map(str::to_string);
                let content_length = response.header("content-length").map(str::to_string);
                let etag = response.header("etag").map(str::to_string);
                let reader = response.into_reader();
                opened_from_parts(
                    ResponseView {
                        status,
                        content_range: content_range.as_deref(),
                        content_length: content_length.as_deref(),
                        etag: etag.as_deref(),
                        last_modified: None,
                    },
                    offset,
                    reader,
                    &url,
                )
            },
            MAX_BODY_BYTES,
            &FAST,
            &|_| {},
            &mut |_, _| true,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(got, payload);
        verify_sha512(&got, &sha512_hex(&payload)).unwrap();
        server.join().unwrap();
    }
}
