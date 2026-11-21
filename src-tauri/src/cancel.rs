//! Request-scoped cancellation for install and repair.
//!
//! The UI makes a request id when the player starts an install and sends it
//! with both `install` and `cancel`. A cancel that lands before the worker has
//! registered its flag is stored under that id, so the worker sees it the
//! moment it registers. Nothing is lost to ordering.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Finished ids remembered so a late cancel for them is ignored, not stored.
const FINISHED_MEMORY: usize = 32;

#[derive(Default)]
struct Inner {
    flags: HashMap<String, Arc<AtomicBool>>,
    running: HashSet<String>,
    finished: VecDeque<String>,
}

#[derive(Default)]
pub struct CancelRegistry {
    inner: Mutex<Inner>,
}

impl CancelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The worker calls this as soon as it starts. Returns the flag for this
    /// request, already set if the player cancelled before the worker got here.
    pub fn register(&self, request_id: &str) -> Arc<AtomicBool> {
        let mut inner = self.lock();
        // Early cancels for requests that never started are dead weight now.
        let Inner { flags, running, .. } = &mut *inner;
        flags.retain(|id, _| id == request_id || running.contains(id));
        let flag = flags
            .entry(request_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        running.insert(request_id.to_string());
        flag
    }

    /// Cancel one request, whether or not its worker has registered yet.
    /// `None` cancels every running request (callers without an id).
    pub fn cancel(&self, request_id: Option<&str>) {
        let mut inner = self.lock();
        match request_id {
            Some(id) => {
                if inner.finished.iter().any(|done| done == id) {
                    return;
                }
                inner
                    .flags
                    .entry(id.to_string())
                    .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                    .store(true, Ordering::SeqCst);
            }
            None => {
                let Inner { flags, running, .. } = &*inner;
                for id in running {
                    if let Some(flag) = flags.get(id) {
                        flag.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    /// The worker is done (success, error, or cancel).
    pub fn finish(&self, request_id: &str) {
        let mut inner = self.lock();
        inner.flags.remove(request_id);
        inner.running.remove(request_id);
        inner.finished.push_back(request_id.to_string());
        while inner.finished.len() > FINISHED_MEMORY {
            inner.finished.pop_front();
        }
    }

    #[cfg(test)]
    fn stored(&self) -> usize {
        self.lock().flags.len()
    }
}

/// Unregisters the request when the worker returns, on every path.
pub struct CancelGuard<'a> {
    registry: &'a CancelRegistry,
    request_id: String,
    pub flag: Arc<AtomicBool>,
}

impl<'a> CancelGuard<'a> {
    pub fn new(registry: &'a CancelRegistry, request_id: String) -> Self {
        let flag = registry.register(&request_id);
        Self {
            registry,
            request_id,
            flag,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        self.registry.finish(&self.request_id);
    }
}

/// An id for callers that did not send one. Unique per process.
pub fn anonymous_request_id() -> String {
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("anon-{}", NEXT.fetch_add(1, Ordering::SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_before_register_is_honoured() {
        let registry = CancelRegistry::new();
        registry.cancel(Some("req-1"));
        let guard = CancelGuard::new(&registry, "req-1".into());
        assert!(guard.is_cancelled());
    }

    #[test]
    fn cancel_after_register_reaches_the_running_flag() {
        let registry = CancelRegistry::new();
        let guard = CancelGuard::new(&registry, "req-1".into());
        assert!(!guard.is_cancelled());
        registry.cancel(Some("req-1"));
        assert!(guard.is_cancelled());
        assert!(guard.flag.load(Ordering::SeqCst));
    }

    #[test]
    fn cancel_is_scoped_to_its_request() {
        let registry = CancelRegistry::new();
        let first = CancelGuard::new(&registry, "req-1".into());
        registry.cancel(Some("req-2"));
        assert!(!first.is_cancelled());
        drop(first);
        // An old request's cancel never leaks into the next install.
        registry.cancel(Some("req-1"));
        let next = CancelGuard::new(&registry, "req-3".into());
        assert!(!next.is_cancelled());
    }

    #[test]
    fn late_cancel_for_a_finished_request_is_not_stored() {
        let registry = CancelRegistry::new();
        drop(CancelGuard::new(&registry, "req-1".into()));
        registry.cancel(Some("req-1"));
        assert_eq!(registry.stored(), 0);
        let again = CancelGuard::new(&registry, "req-1".into());
        assert!(!again.is_cancelled());
    }

    #[test]
    fn stale_early_cancels_are_pruned_on_the_next_register() {
        let registry = CancelRegistry::new();
        registry.cancel(Some("never-started"));
        let guard = CancelGuard::new(&registry, "req-2".into());
        assert_eq!(registry.stored(), 1);
        assert!(!guard.is_cancelled());
    }

    #[test]
    fn cancel_without_an_id_stops_running_requests_only() {
        let registry = CancelRegistry::new();
        let running = CancelGuard::new(&registry, "req-1".into());
        registry.cancel(None);
        assert!(running.is_cancelled());
        drop(running);
        let next = CancelGuard::new(&registry, "req-2".into());
        assert!(!next.is_cancelled());
    }

    #[test]
    fn concurrent_cancel_and_register_never_lose_the_cancel() {
        for round in 0..200 {
            let registry = Arc::new(CancelRegistry::new());
            let id = format!("req-{round}");
            let canceller = {
                let registry = registry.clone();
                let id = id.clone();
                std::thread::spawn(move || registry.cancel(Some(&id)))
            };
            let flag = registry.register(&id);
            canceller.join().unwrap();
            assert!(flag.load(Ordering::SeqCst), "round {round} lost the cancel");
        }
    }

    #[test]
    fn anonymous_ids_are_unique() {
        assert_ne!(anonymous_request_id(), anonymous_request_id());
    }
}
