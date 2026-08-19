use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorEmission {
    Emit,
    Suppress,
    EmitWithSummary { suppressed: u64 },
}

#[derive(Debug, Clone, Copy)]
struct ErrorBucket {
    last_emitted_at: Instant,
    suppressed: u64,
}

/// Coalesces repeated stable error codes without polling or background timers.
/// Callers still decide which structured fields are safe to emit.
#[derive(Debug)]
pub struct RepeatedErrorAggregator {
    window: Duration,
    buckets: Mutex<HashMap<&'static str, ErrorBucket>>,
}

impl RepeatedErrorAggregator {
    pub fn new(window: Duration) -> Self {
        assert!(!window.is_zero(), "aggregation window must be non-zero");
        Self {
            window,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn observe(&self, error_code: &'static str) -> ErrorEmission {
        self.observe_at(error_code, Instant::now())
    }

    pub fn observe_at(&self, error_code: &'static str, now: Instant) -> ErrorEmission {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(bucket) = buckets.get_mut(error_code) else {
            buckets.insert(
                error_code,
                ErrorBucket {
                    last_emitted_at: now,
                    suppressed: 0,
                },
            );
            return ErrorEmission::Emit;
        };

        if now.saturating_duration_since(bucket.last_emitted_at) < self.window {
            bucket.suppressed = bucket.suppressed.saturating_add(1);
            return ErrorEmission::Suppress;
        }

        let suppressed = bucket.suppressed;
        bucket.last_emitted_at = now;
        bucket.suppressed = 0;
        if suppressed == 0 {
            ErrorEmission::Emit
        } else {
            ErrorEmission::EmitWithSummary { suppressed }
        }
    }

    /// Returns pending counts for shutdown/audit flushing without starting a timer.
    pub fn drain_summaries(&self) -> Vec<(&'static str, u64)> {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut summaries: Vec<_> = buckets
            .drain()
            .filter_map(|(code, bucket)| {
                (bucket.suppressed > 0).then_some((code, bucket.suppressed))
            })
            .collect();
        summaries.sort_unstable_by_key(|(code, _)| *code);
        summaries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_once_then_summarizes_repetitions_after_the_window() {
        let start = Instant::now();
        let aggregator = RepeatedErrorAggregator::new(Duration::from_secs(60));

        assert_eq!(
            aggregator.observe_at("backend_timeout", start),
            ErrorEmission::Emit
        );
        assert_eq!(
            aggregator.observe_at("backend_timeout", start + Duration::from_secs(1)),
            ErrorEmission::Suppress
        );
        assert_eq!(
            aggregator.observe_at("backend_timeout", start + Duration::from_secs(2)),
            ErrorEmission::Suppress
        );
        assert_eq!(
            aggregator.observe_at("backend_timeout", start + Duration::from_secs(60)),
            ErrorEmission::EmitWithSummary { suppressed: 2 }
        );
    }

    #[test]
    fn keeps_error_codes_independent_and_drains_without_a_timer() {
        let start = Instant::now();
        let aggregator = RepeatedErrorAggregator::new(Duration::from_secs(60));

        aggregator.observe_at("backend_timeout", start);
        aggregator.observe_at("backend_timeout", start);
        aggregator.observe_at("database_busy", start);
        aggregator.observe_at("database_busy", start);
        aggregator.observe_at("database_busy", start);

        assert_eq!(
            aggregator.drain_summaries(),
            vec![("backend_timeout", 1), ("database_busy", 2)]
        );
        assert!(aggregator.drain_summaries().is_empty());
    }
}
