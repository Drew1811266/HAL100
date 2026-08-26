use std::sync::{Arc, Mutex};

use hal100_core::AgentTaskAdjudicationOutcome;
use hal100_protocol::{
    AgentIntentShadowAdjudicationOutcome, AgentIntentShadowMetrics,
    AgentIntentShadowProposalStatus, AgentTaskRoutingDecision, AgentTaskRoutingMetrics,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct AgentIntentShadowObservation {
    pub proposal_status: AgentIntentShadowProposalStatus,
    pub adjudication_outcome: Option<AgentTaskAdjudicationOutcome>,
    pub pi_latency_ms: Option<u64>,
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct AgentIntentShadowObserver {
    metrics: Arc<Mutex<AgentIntentShadowMetrics>>,
}

impl AgentIntentShadowObserver {
    pub(super) fn record(&self, observation: AgentIntentShadowObservation) {
        let mut metrics = self
            .metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        metrics.sample_count = metrics.sample_count.saturating_add(1);
        metrics.last_proposal_status = Some(observation.proposal_status);
        metrics.updated_at_ms = Some(observation.observed_at_ms);

        match observation.proposal_status {
            AgentIntentShadowProposalStatus::NotRequested => {
                metrics.deterministic_resolved_count =
                    metrics.deterministic_resolved_count.saturating_add(1);
            }
            AgentIntentShadowProposalStatus::Proposed => {
                metrics.pi_requested_count = metrics.pi_requested_count.saturating_add(1);
                metrics.pi_proposed_count = metrics.pi_proposed_count.saturating_add(1);
            }
            AgentIntentShadowProposalStatus::Invalid => {
                metrics.pi_requested_count = metrics.pi_requested_count.saturating_add(1);
                metrics.pi_invalid_count = metrics.pi_invalid_count.saturating_add(1);
            }
            AgentIntentShadowProposalStatus::Failed => {
                metrics.pi_requested_count = metrics.pi_requested_count.saturating_add(1);
                metrics.pi_failed_count = metrics.pi_failed_count.saturating_add(1);
            }
            AgentIntentShadowProposalStatus::Rejected => {
                metrics.pi_requested_count = metrics.pi_requested_count.saturating_add(1);
                metrics.pi_rejected_count = metrics.pi_rejected_count.saturating_add(1);
            }
            AgentIntentShadowProposalStatus::ProtocolError => {
                metrics.pi_requested_count = metrics.pi_requested_count.saturating_add(1);
                metrics.pi_protocol_error_count = metrics.pi_protocol_error_count.saturating_add(1);
            }
        }

        if let Some(latency_ms) = observation.pi_latency_ms {
            metrics.cumulative_pi_latency_ms =
                metrics.cumulative_pi_latency_ms.saturating_add(latency_ms);
            metrics.max_pi_latency_ms = metrics.max_pi_latency_ms.max(latency_ms);
            metrics.last_pi_latency_ms = Some(latency_ms);
        }

        let Some(outcome) = observation.adjudication_outcome else {
            return;
        };
        let wire_outcome = match outcome {
            AgentTaskAdjudicationOutcome::Agreement => {
                metrics.agreement_count = metrics.agreement_count.saturating_add(1);
                AgentIntentShadowAdjudicationOutcome::Agreement
            }
            AgentTaskAdjudicationOutcome::DeterministicGuard => {
                metrics.deterministic_guard_count =
                    metrics.deterministic_guard_count.saturating_add(1);
                AgentIntentShadowAdjudicationOutcome::DeterministicGuard
            }
            AgentTaskAdjudicationOutcome::DeterministicOnly => {
                metrics.deterministic_only_count =
                    metrics.deterministic_only_count.saturating_add(1);
                AgentIntentShadowAdjudicationOutcome::DeterministicOnly
            }
            AgentTaskAdjudicationOutcome::ProposalCandidate => {
                metrics.proposal_candidate_count =
                    metrics.proposal_candidate_count.saturating_add(1);
                AgentIntentShadowAdjudicationOutcome::ProposalCandidate
            }
            AgentTaskAdjudicationOutcome::Conflict => {
                metrics.conflict_count = metrics.conflict_count.saturating_add(1);
                AgentIntentShadowAdjudicationOutcome::Conflict
            }
            AgentTaskAdjudicationOutcome::Unresolved => {
                metrics.unresolved_count = metrics.unresolved_count.saturating_add(1);
                AgentIntentShadowAdjudicationOutcome::Unresolved
            }
        };
        metrics.last_adjudication_outcome = Some(wire_outcome);
    }

    pub(super) fn snapshot(&self) -> AgentIntentShadowMetrics {
        self.metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct AgentTaskRoutingObserver {
    metrics: Arc<Mutex<AgentTaskRoutingMetrics>>,
}

impl AgentTaskRoutingObserver {
    pub(super) fn record(&self, decision: AgentTaskRoutingDecision, observed_at_ms: i64) {
        let mut metrics = self
            .metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        metrics.sample_count = metrics.sample_count.saturating_add(1);
        match decision {
            AgentTaskRoutingDecision::StructuredDeterministic => {
                metrics.structured_deterministic_count =
                    metrics.structured_deterministic_count.saturating_add(1);
            }
            AgentTaskRoutingDecision::StructuredPi => {
                metrics.structured_pi_count = metrics.structured_pi_count.saturating_add(1);
            }
            AgentTaskRoutingDecision::GuardedResponse => {
                metrics.guarded_response_count = metrics.guarded_response_count.saturating_add(1);
            }
            AgentTaskRoutingDecision::SafeLegacyDeterministic => {
                metrics.safe_legacy_deterministic_count =
                    metrics.safe_legacy_deterministic_count.saturating_add(1);
            }
            AgentTaskRoutingDecision::LegacyNoToolFallback => {
                metrics.legacy_no_tool_fallback_count =
                    metrics.legacy_no_tool_fallback_count.saturating_add(1);
            }
            AgentTaskRoutingDecision::FailClosed => {
                metrics.fail_closed_count = metrics.fail_closed_count.saturating_add(1);
            }
        }
        metrics.last_decision = Some(decision);
        metrics.updated_at_ms = Some(observed_at_ms);
    }

    pub(super) fn snapshot(&self) -> AgentTaskRoutingMetrics {
        self.metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_aggregates_only_bounded_status_and_latency() {
        let observer = AgentIntentShadowObserver::default();
        observer.record(AgentIntentShadowObservation {
            proposal_status: AgentIntentShadowProposalStatus::NotRequested,
            adjudication_outcome: Some(AgentTaskAdjudicationOutcome::DeterministicOnly),
            pi_latency_ms: None,
            observed_at_ms: 10,
        });
        observer.record(AgentIntentShadowObservation {
            proposal_status: AgentIntentShadowProposalStatus::Proposed,
            adjudication_outcome: Some(AgentTaskAdjudicationOutcome::ProposalCandidate),
            pi_latency_ms: Some(42),
            observed_at_ms: 20,
        });

        let metrics = observer.snapshot();
        assert_eq!(metrics.sample_count, 2);
        assert_eq!(metrics.deterministic_resolved_count, 1);
        assert_eq!(metrics.pi_requested_count, 1);
        assert_eq!(metrics.pi_proposed_count, 1);
        assert_eq!(metrics.proposal_candidate_count, 1);
        assert_eq!(metrics.cumulative_pi_latency_ms, 42);
        assert_eq!(metrics.max_pi_latency_ms, 42);
        assert_eq!(metrics.updated_at_ms, Some(20));
    }

    #[test]
    fn routing_observer_counts_only_bounded_activation_decisions() {
        let observer = AgentTaskRoutingObserver::default();
        observer.record(AgentTaskRoutingDecision::StructuredPi, 30);
        observer.record(AgentTaskRoutingDecision::FailClosed, 40);

        let metrics = observer.snapshot();
        assert_eq!(metrics.sample_count, 2);
        assert_eq!(metrics.structured_pi_count, 1);
        assert_eq!(metrics.fail_closed_count, 1);
        assert_eq!(
            metrics.last_decision,
            Some(AgentTaskRoutingDecision::FailClosed)
        );
        assert_eq!(metrics.updated_at_ms, Some(40));
    }
}
