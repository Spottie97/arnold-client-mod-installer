use std::time::{Duration, Instant};

use crate::install::ProgressEvent;

/// Minimum time between two running updates that did not move the percent.
pub const PROGRESS_MIN_GAP: Duration = Duration::from_millis(150);

/// Drops noisy running updates. Terminal statuses and step/file changes always pass.
pub struct ProgressThrottle {
    min_gap: Duration,
    last_at: Option<Instant>,
    last_key: Option<(String, Option<String>, String)>,
    last_percent: u8,
}

impl ProgressThrottle {
    pub fn new(min_gap: Duration) -> Self {
        Self {
            min_gap,
            last_at: None,
            last_key: None,
            last_percent: 0,
        }
    }

    /// Returns whether `event` should be sent now. Dropped events are coalesced:
    /// the next event that passes carries the latest percent and message.
    #[must_use]
    pub fn should_emit(&mut self, event: &ProgressEvent, now: Instant) -> bool {
        let terminal = event.status != "running";
        let key_changed = match &self.last_key {
            Some((step, file, status)) => {
                step != &event.step || file != &event.file || status != &event.status
            }
            None => true,
        };
        let gap_elapsed = match self.last_at {
            Some(last) => now.saturating_duration_since(last) >= self.min_gap,
            None => true,
        };
        let percent_moved = event.percent >= self.last_percent.saturating_add(1);
        if !(terminal || key_changed || gap_elapsed || percent_moved) {
            return false;
        }
        self.last_at = Some(now);
        self.last_key = Some((event.step.clone(), event.file.clone(), event.status.clone()));
        self.last_percent = event.percent;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(step: &str, file: Option<&str>, status: &str, percent: u8) -> ProgressEvent {
        ProgressEvent {
            run_id: "run".into(),
            step: step.into(),
            file: file.map(str::to_string),
            status: status.into(),
            percent,
            message: "m".into(),
        }
    }

    #[test]
    fn running_updates_inside_the_gap_and_under_one_percent_are_dropped() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::new(PROGRESS_MIN_GAP);
        let event = sample("download", Some("lootr.jar"), "running", 13);
        assert!(throttle.should_emit(&event, start));
        assert!(!throttle.should_emit(&event, start + Duration::from_millis(149)));
        assert!(!throttle.should_emit(&event, start));
    }

    #[test]
    fn same_key_emits_after_the_minimum_gap() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::new(PROGRESS_MIN_GAP);
        let event = sample("download", Some("lootr.jar"), "running", 13);
        assert!(throttle.should_emit(&event, start));
        assert!(throttle.should_emit(&event, start + PROGRESS_MIN_GAP));
    }

    #[test]
    fn one_percent_change_emits_immediately() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::new(PROGRESS_MIN_GAP);
        let event = sample("download", Some("lootr.jar"), "running", 13);
        assert!(throttle.should_emit(&event, start));
        let mut next = event.clone();
        next.percent = 14;
        assert!(throttle.should_emit(&next, start));
    }

    #[test]
    fn step_or_file_change_always_emits() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::new(PROGRESS_MIN_GAP);
        assert!(throttle.should_emit(&sample("prepare", None, "running", 5), start));
        assert!(throttle.should_emit(&sample("download", Some("a.jar"), "running", 5), start));
        assert!(throttle.should_emit(&sample("write", Some("a.jar"), "running", 5), start));
        assert!(throttle.should_emit(&sample("write", Some("b.jar"), "running", 5), start));
    }

    #[test]
    fn terminal_status_always_emits() {
        let start = Instant::now();
        for status in ["done", "error", "cancelled"] {
            let mut throttle = ProgressThrottle::new(PROGRESS_MIN_GAP);
            assert!(throttle.should_emit(&sample("download", Some("a.jar"), "running", 40), start));
            let terminal = sample("download", Some("a.jar"), status, 40);
            assert!(throttle.should_emit(&terminal, start));
            assert!(throttle.should_emit(&terminal, start));
        }
    }
}
