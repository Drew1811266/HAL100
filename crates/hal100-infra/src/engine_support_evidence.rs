use hal100_protocol::{
    InferenceEngineKind, InferenceEngineSupportEvidenceSummary, InferenceEngineSupportStatus,
};

/// Summarize the evidence already justified by the registered support cell.
///
/// A connected adapter has a real software contract and a bounded protocol fixture, but that
/// evidence is deliberately not promoted into platform or deployment support. A formal cell is
/// only emitted by code after its real-service acceptance and runtime-profile evidence has been
/// recorded. This function is intentionally conservative and contains no environment inference.
pub fn support_evidence_for(
    _engine: InferenceEngineKind,
    status: Option<InferenceEngineSupportStatus>,
) -> InferenceEngineSupportEvidenceSummary {
    InferenceEngineSupportEvidenceSummary::for_status(
        status.unwrap_or(InferenceEngineSupportStatus::Reserved),
    )
}

#[cfg(test)]
mod tests {
    use hal100_protocol::{InferenceEngineKind, InferenceEngineSupportEvidenceKind};

    use super::*;

    #[test]
    fn connected_cells_stop_at_software_and_protocol_evidence() {
        let summary = support_evidence_for(
            InferenceEngineKind::TensorRtLlm,
            Some(InferenceEngineSupportStatus::Connected),
        );
        assert_eq!(summary.verified.len(), 2);
        assert_eq!(summary.missing.len(), 5);
        assert_eq!(
            summary.missing[0],
            InferenceEngineSupportEvidenceKind::PlatformRuntime
        );
    }

    #[test]
    fn formal_cells_have_no_unverified_evidence_debt() {
        for status in [
            InferenceEngineSupportStatus::Managed,
            InferenceEngineSupportStatus::VerifiedExternal,
        ] {
            let summary = support_evidence_for(InferenceEngineKind::MlxLm, Some(status));
            assert_eq!(summary.verified.len(), 7);
            assert!(summary.missing.is_empty());
        }
    }
}
