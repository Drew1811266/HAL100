use hal100_protocol::{
    EngineHostCompatibility, EngineHostCompatibilityIssue, InferenceEngineOwnership,
    InferenceEngineRecommendation, InferenceEngineRecommendationReason,
    InferenceEngineSupportStatus,
};

/// Produce a stable, explainable ranking for the current host.
///
/// The ranking is deliberately advisory: `eligible` is derived from the same manifest support
/// cell and formal-status gate used by runtime profiles, while activation still performs a fresh
/// target/evidence check. Observed external runtimes improve ordering but never bypass a mismatch
/// or promote a `connected` cell.
pub fn recommendation_for(
    compatibility: &EngineHostCompatibility,
    ownership: InferenceEngineOwnership,
    observed_runtime_count: usize,
) -> InferenceEngineRecommendation {
    if !compatibility.compatible {
        if compatibility
            .issues
            .contains(&EngineHostCompatibilityIssue::SupportCellAmbiguous)
        {
            return InferenceEngineRecommendation {
                eligible: false,
                score: 0,
                reasons: vec![InferenceEngineRecommendationReason::SupportCellAmbiguous],
            };
        }
        let support_only = !compatibility.issues.is_empty()
            && compatibility
                .issues
                .iter()
                .all(|issue| matches!(issue, EngineHostCompatibilityIssue::SupportNotFormal));
        return InferenceEngineRecommendation {
            eligible: false,
            score: 0,
            reasons: if support_only {
                vec![
                    InferenceEngineRecommendationReason::ConnectedOnly,
                    InferenceEngineRecommendationReason::ProtocolRequiresExplicitQualification,
                ]
            } else {
                vec![InferenceEngineRecommendationReason::HostMismatch]
            },
        };
    }

    let mut score = 40u16;
    let mut reasons = vec![InferenceEngineRecommendationReason::HostCompatible];
    match compatibility.support_status {
        Some(InferenceEngineSupportStatus::Managed) => {
            score += 40;
            reasons.push(InferenceEngineRecommendationReason::ManagedLifecycle);
            reasons.push(InferenceEngineRecommendationReason::FormalSupport);
        }
        Some(InferenceEngineSupportStatus::VerifiedExternal) => {
            score += 30;
            reasons.push(InferenceEngineRecommendationReason::FormalSupport);
        }
        Some(InferenceEngineSupportStatus::Connected) => {
            score += 10;
            reasons.push(InferenceEngineRecommendationReason::ConnectedOnly);
            reasons
                .push(InferenceEngineRecommendationReason::ProtocolRequiresExplicitQualification);
        }
        Some(InferenceEngineSupportStatus::Reserved) | None => {
            reasons
                .push(InferenceEngineRecommendationReason::ProtocolRequiresExplicitQualification);
        }
    }
    if ownership == InferenceEngineOwnership::Managed {
        score = score.saturating_add(5);
    }
    if observed_runtime_count > 0 {
        score = score.saturating_add(5);
        reasons.push(InferenceEngineRecommendationReason::VerifiedRuntimeObserved);
    }
    InferenceEngineRecommendation {
        eligible: compatibility.compatible,
        score,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use hal100_protocol::{
        EngineHostCompatibilityIssue, InferenceAccelerator, InferenceEngineKind,
    };

    use super::*;

    fn compatibility(
        compatible: bool,
        status: Option<InferenceEngineSupportStatus>,
    ) -> EngineHostCompatibility {
        EngineHostCompatibility {
            engine: InferenceEngineKind::LlamaCpp,
            compatible,
            matched_accelerators: vec![InferenceAccelerator::Metal],
            support_status: status,
            support_evidence: None,
            issues: if compatible {
                Vec::new()
            } else {
                vec![EngineHostCompatibilityIssue::SupportNotFormal]
            },
        }
    }

    #[test]
    fn managed_runtime_ranks_above_external_runtime() {
        let managed = recommendation_for(
            &compatibility(true, Some(InferenceEngineSupportStatus::Managed)),
            InferenceEngineOwnership::Managed,
            0,
        );
        let external = recommendation_for(
            &compatibility(true, Some(InferenceEngineSupportStatus::VerifiedExternal)),
            InferenceEngineOwnership::External,
            1,
        );
        assert!(managed.eligible);
        assert!(managed.score > external.score);
    }

    #[test]
    fn connected_runtime_is_explained_but_not_eligible() {
        let connected = recommendation_for(
            &compatibility(false, Some(InferenceEngineSupportStatus::Connected)),
            InferenceEngineOwnership::External,
            1,
        );
        assert!(!connected.eligible);
        assert_eq!(connected.score, 0);
        assert_eq!(
            connected.reasons,
            vec![
                InferenceEngineRecommendationReason::ConnectedOnly,
                InferenceEngineRecommendationReason::ProtocolRequiresExplicitQualification,
            ]
        );
    }

    #[test]
    fn mixed_support_cells_are_not_recommended_without_selection() {
        let recommendation = recommendation_for(
            &EngineHostCompatibility {
                engine: InferenceEngineKind::OpenVino,
                compatible: false,
                matched_accelerators: vec![
                    InferenceAccelerator::Cpu,
                    InferenceAccelerator::IntelGpu,
                ],
                support_status: Some(InferenceEngineSupportStatus::VerifiedExternal),
                support_evidence: None,
                issues: vec![EngineHostCompatibilityIssue::SupportCellAmbiguous],
            },
            InferenceEngineOwnership::External,
            1,
        );

        assert!(!recommendation.eligible);
        assert_eq!(recommendation.score, 0);
        assert_eq!(
            recommendation.reasons,
            vec![InferenceEngineRecommendationReason::SupportCellAmbiguous]
        );
    }
}
