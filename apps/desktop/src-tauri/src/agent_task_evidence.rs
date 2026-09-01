use hal100_core::{AgentTaskKind, AgentTaskSpec, AgentTaskSuccessPredicate};
use hal100_protocol::{
    AgentExternalIntegrationStatus, AgentRuntimeCatalog, AgentTaskEvidenceSource,
    AgentTaskVerificationState, EngineInstallState, EngineRuntimeState,
    EnvironmentDiagnosticReport, EnvironmentHealthStatus, ExternalAgentIntegrationState,
    RemoteModelRepository, RemoteModelSearchResults,
};

/// A reduced, bounded semantic observation. It never retains tool output, resource identifiers,
/// paths, credentials, prompts, answers, or plan authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AgentTaskEvidence {
    pub(super) verification_state: AgentTaskVerificationState,
    pub(super) source: Option<AgentTaskEvidenceSource>,
    pub(super) observation_count: u8,
}

impl AgentTaskEvidence {
    pub(super) const fn satisfied(source: AgentTaskEvidenceSource) -> Self {
        Self::observed(AgentTaskVerificationState::Satisfied, source)
    }

    pub(super) const fn unsatisfied(source: AgentTaskEvidenceSource) -> Self {
        Self::observed(AgentTaskVerificationState::Unsatisfied, source)
    }

    pub(super) const fn unavailable(source: Option<AgentTaskEvidenceSource>) -> Self {
        Self {
            verification_state: AgentTaskVerificationState::EvidenceUnavailable,
            source,
            observation_count: if source.is_some() { 1 } else { 0 },
        }
    }

    pub(super) const fn pending_action_plan() -> Self {
        Self::observed(
            AgentTaskVerificationState::Pending,
            AgentTaskEvidenceSource::ActionPlan,
        )
    }

    pub(super) const fn is_satisfied(self) -> bool {
        matches!(
            self.verification_state,
            AgentTaskVerificationState::Satisfied
        )
    }

    const fn observed(
        verification_state: AgentTaskVerificationState,
        source: AgentTaskEvidenceSource,
    ) -> Self {
        Self {
            verification_state,
            source: Some(source),
            observation_count: 1,
        }
    }
}

pub(super) enum AgentTaskToolObservation<'a> {
    SystemSummary,
    RuntimeCatalog(&'a AgentRuntimeCatalog),
    EnvironmentDiagnostics(&'a EnvironmentDiagnosticReport),
    OperationalHistory,
    OperationalHealth,
    ModelCatalog(&'a RemoteModelSearchResults),
    ModelRepository(&'a RemoteModelRepository),
    ExternalIntegration(&'a AgentExternalIntegrationStatus),
    DownloadPlan,
}

pub(super) fn evaluate_tool_observation(
    spec: &AgentTaskSpec,
    observation: AgentTaskToolObservation<'_>,
) -> Option<AgentTaskEvidence> {
    let predicate = spec.success_predicate();
    match observation {
        AgentTaskToolObservation::SystemSummary
            if spec.task_kind() == AgentTaskKind::InspectSystem =>
        {
            Some(AgentTaskEvidence::satisfied(
                AgentTaskEvidenceSource::SystemProbe,
            ))
        }
        AgentTaskToolObservation::RuntimeCatalog(catalog) => {
            evaluate_runtime_observation(spec, predicate, catalog)
        }
        AgentTaskToolObservation::EnvironmentDiagnostics(report) => {
            evaluate_environment_observation(predicate, report)
        }
        AgentTaskToolObservation::OperationalHistory
            if spec.task_kind() == AgentTaskKind::AnalyzeOperationalHistory =>
        {
            Some(AgentTaskEvidence::satisfied(
                AgentTaskEvidenceSource::OperationalHistory,
            ))
        }
        AgentTaskToolObservation::OperationalHealth
            if spec.task_kind() == AgentTaskKind::ObserveDeploymentHealth =>
        {
            Some(AgentTaskEvidence::satisfied(
                AgentTaskEvidenceSource::OperationalHealth,
            ))
        }
        AgentTaskToolObservation::ModelCatalog(results)
            if predicate == AgentTaskSuccessPredicate::CatalogResultsAvailable =>
        {
            Some(observed_state(
                !results.items.is_empty(),
                AgentTaskEvidenceSource::ModelCatalog,
            ))
        }
        AgentTaskToolObservation::ModelRepository(repository)
            if predicate == AgentTaskSuccessPredicate::RepositoryInspected =>
        {
            Some(observed_state(
                !repository.files.is_empty(),
                AgentTaskEvidenceSource::ModelRepository,
            ))
        }
        AgentTaskToolObservation::ExternalIntegration(status) => {
            evaluate_external_observation(predicate, status)
        }
        AgentTaskToolObservation::DownloadPlan
            if predicate == AgentTaskSuccessPredicate::DownloadPlanCreated =>
        {
            Some(AgentTaskEvidence::satisfied(
                AgentTaskEvidenceSource::ActionPlan,
            ))
        }
        _ => None,
    }
}

fn evaluate_runtime_observation(
    spec: &AgentTaskSpec,
    predicate: AgentTaskSuccessPredicate,
    catalog: &AgentRuntimeCatalog,
) -> Option<AgentTaskEvidence> {
    let source = AgentTaskEvidenceSource::RuntimeCatalog;
    match predicate {
        AgentTaskSuccessPredicate::EvidenceCollected
            if spec.task_kind() == AgentTaskKind::InspectRuntime =>
        {
            Some(AgentTaskEvidence::satisfied(source))
        }
        AgentTaskSuccessPredicate::RuntimeModelActive => {
            let target = spec.target().resource_id();
            Some(match target {
                Some(target) => observed_state(
                    catalog.active_model_id.as_deref() == Some(target)
                        && catalog.engine_runtime_state == EngineRuntimeState::Running,
                    source,
                ),
                None => AgentTaskEvidence::unavailable(Some(source)),
            })
        }
        AgentTaskSuccessPredicate::RuntimeModelStopped => Some(observed_state(
            catalog.engine_runtime_state == EngineRuntimeState::Stopped
                && catalog.active_model_id.is_none(),
            source,
        )),
        AgentTaskSuccessPredicate::ModelAbsent => {
            let target = spec.target().resource_id();
            Some(match target {
                Some(target) => observed_state(
                    !catalog.models.iter().any(|model| model.id == target),
                    source,
                ),
                None => AgentTaskEvidence::unavailable(Some(source)),
            })
        }
        AgentTaskSuccessPredicate::EngineInstalled => Some(observed_state(
            catalog.engine_install_state == EngineInstallState::Installed,
            source,
        )),
        AgentTaskSuccessPredicate::EngineAbsent => Some(observed_state(
            catalog.engine_install_state == EngineInstallState::NotInstalled,
            source,
        )),
        _ => None,
    }
}

fn evaluate_environment_observation(
    predicate: AgentTaskSuccessPredicate,
    report: &EnvironmentDiagnosticReport,
) -> Option<AgentTaskEvidence> {
    let source = AgentTaskEvidenceSource::EnvironmentDiagnostics;
    match predicate {
        AgentTaskSuccessPredicate::EnvironmentDiagnosed => {
            Some(AgentTaskEvidence::satisfied(source))
        }
        AgentTaskSuccessPredicate::RepairFindingResolved => {
            if report.status == EnvironmentHealthStatus::Healthy {
                Some(AgentTaskEvidence::satisfied(source))
            } else if report
                .findings
                .iter()
                .any(|finding| finding.repair_kind.is_some())
            {
                Some(AgentTaskEvidence::unsatisfied(source))
            } else {
                Some(AgentTaskEvidence::unavailable(Some(source)))
            }
        }
        _ => None,
    }
}

fn evaluate_external_observation(
    predicate: AgentTaskSuccessPredicate,
    status: &AgentExternalIntegrationStatus,
) -> Option<AgentTaskEvidence> {
    let source = AgentTaskEvidenceSource::ExternalIntegrationStatus;
    match predicate {
        AgentTaskSuccessPredicate::EvidenceCollected => Some(AgentTaskEvidence::satisfied(source)),
        AgentTaskSuccessPredicate::IntegrationConfigured => Some(observed_state(
            status.integration_state == ExternalAgentIntegrationState::Configured
                && status.configured_protocol.is_some(),
            source,
        )),
        AgentTaskSuccessPredicate::IntegrationDisconnected => Some(observed_state(
            matches!(
                status.integration_state,
                ExternalAgentIntegrationState::NotInstalled
                    | ExternalAgentIntegrationState::InstalledNotConfigured
            ) && status.configured_protocol.is_none(),
            source,
        )),
        AgentTaskSuccessPredicate::ManagedInstallationPresent => {
            Some(observed_state(status.managed_installation, source))
        }
        AgentTaskSuccessPredicate::ManagedInstallationAbsent => {
            Some(observed_state(!status.managed_installation, source))
        }
        _ => None,
    }
}

const fn observed_state(satisfied: bool, source: AgentTaskEvidenceSource) -> AgentTaskEvidence {
    if satisfied {
        AgentTaskEvidence::satisfied(source)
    } else {
        AgentTaskEvidence::unsatisfied(source)
    }
}

#[cfg(test)]
mod tests {
    use hal100_core::{
        AgentTaskKind, AgentTaskProviderMode, AgentTaskSpec, AgentTaskTarget,
        ExternalAgentIntegrationId,
    };
    use hal100_protocol::{
        AgentRuntimeModel, DiagnosticComponent, DiagnosticRepairKind, DiagnosticSeverity,
        DownloadSource, EnvironmentDiagnosticFinding, ExternalAgentGatewayProtocol,
        OpenCodeIntegrationState,
    };

    use super::*;

    #[test]
    fn runtime_predicate_requires_the_exact_active_model_and_running_engine() {
        let spec = AgentTaskSpec::new(
            AgentTaskKind::StartModel,
            AgentTaskTarget::model(Some("model-a".to_owned())).expect("model target"),
            AgentTaskProviderMode::Local,
        )
        .expect("task spec");
        let mut catalog = runtime_catalog();
        catalog.active_model_id = Some("model-a".to_owned());
        catalog.engine_runtime_state = EngineRuntimeState::Running;
        assert_eq!(
            evaluate_tool_observation(&spec, AgentTaskToolObservation::RuntimeCatalog(&catalog))
                .expect("evidence")
                .verification_state,
            AgentTaskVerificationState::Satisfied
        );

        catalog.active_model_id = Some("model-b".to_owned());
        assert_eq!(
            evaluate_tool_observation(&spec, AgentTaskToolObservation::RuntimeCatalog(&catalog))
                .expect("evidence")
                .verification_state,
            AgentTaskVerificationState::Unsatisfied
        );
    }

    #[test]
    fn stopped_model_predicate_requires_stopped_runtime_and_no_active_model() {
        let spec = AgentTaskSpec::new(
            AgentTaskKind::StopModel,
            AgentTaskTarget::model(None).expect("model target"),
            AgentTaskProviderMode::Local,
        )
        .expect("stop task spec");
        let mut catalog = runtime_catalog();
        catalog.engine_runtime_state = EngineRuntimeState::Stopped;
        catalog.active_model_id = None;
        assert_eq!(
            evaluate_tool_observation(&spec, AgentTaskToolObservation::RuntimeCatalog(&catalog))
                .expect("evidence")
                .verification_state,
            AgentTaskVerificationState::Satisfied
        );

        catalog.engine_runtime_state = EngineRuntimeState::Running;
        catalog.active_model_id = Some("model-a".to_owned());
        assert_eq!(
            evaluate_tool_observation(&spec, AgentTaskToolObservation::RuntimeCatalog(&catalog))
                .expect("evidence")
                .verification_state,
            AgentTaskVerificationState::Unsatisfied
        );
    }

    #[test]
    fn repair_predicate_distinguishes_healthy_repairable_and_unavailable_evidence() {
        let spec = AgentTaskSpec::new(
            AgentTaskKind::RepairEnvironment,
            AgentTaskTarget::environment(),
            AgentTaskProviderMode::Local,
        )
        .expect("repair spec");
        let mut report = diagnostic_report(EnvironmentHealthStatus::Healthy, None);
        assert_state(&spec, &report, AgentTaskVerificationState::Satisfied);

        report = diagnostic_report(
            EnvironmentHealthStatus::Error,
            Some(DiagnosticRepairKind::InstallLlamaCpp),
        );
        assert_state(&spec, &report, AgentTaskVerificationState::Unsatisfied);

        report = diagnostic_report(EnvironmentHealthStatus::NeedsAttention, None);
        assert_state(
            &spec,
            &report,
            AgentTaskVerificationState::EvidenceUnavailable,
        );
    }

    #[test]
    fn empty_catalog_is_unsatisfied_and_external_states_are_not_inferred_from_text() {
        let catalog_spec = AgentTaskSpec::new(
            AgentTaskKind::SearchModelCatalog,
            AgentTaskTarget::model_catalog(),
            AgentTaskProviderMode::Local,
        )
        .expect("catalog spec");
        let empty = RemoteModelSearchResults {
            source: DownloadSource::HuggingFace,
            query: "qwen".to_owned(),
            items: Vec::new(),
        };
        assert_eq!(
            evaluate_tool_observation(
                &catalog_spec,
                AgentTaskToolObservation::ModelCatalog(&empty)
            )
            .expect("catalog evidence")
            .verification_state,
            AgentTaskVerificationState::Unsatisfied
        );

        let configure_spec = external_spec(AgentTaskKind::ConfigureExternalAgent);
        let mut status = external_status(ExternalAgentIntegrationState::Configured, true, false);
        assert_eq!(
            evaluate_tool_observation(
                &configure_spec,
                AgentTaskToolObservation::ExternalIntegration(&status)
            )
            .expect("configured evidence")
            .verification_state,
            AgentTaskVerificationState::Satisfied
        );
        status.integration_state = ExternalAgentIntegrationState::Conflict;
        assert_eq!(
            evaluate_tool_observation(
                &configure_spec,
                AgentTaskToolObservation::ExternalIntegration(&status)
            )
            .expect("conflict evidence")
            .verification_state,
            AgentTaskVerificationState::Unsatisfied
        );

        let managed_spec = external_spec(AgentTaskKind::InstallManagedExternalAgent);
        assert_eq!(
            evaluate_tool_observation(
                &managed_spec,
                AgentTaskToolObservation::ExternalIntegration(&external_status(
                    ExternalAgentIntegrationState::InstalledNotConfigured,
                    false,
                    true,
                ))
            )
            .expect("managed evidence")
            .verification_state,
            AgentTaskVerificationState::Satisfied
        );
    }

    fn runtime_catalog() -> AgentRuntimeCatalog {
        AgentRuntimeCatalog {
            engine_install_state: EngineInstallState::Installed,
            engine_runtime_state: EngineRuntimeState::Stopped,
            active_model_id: None,
            active_model_name: None,
            active_backend_id: None,
            configured_backend_count: 0,
            models: vec![AgentRuntimeModel {
                id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                quantization: None,
                size_bytes: 1,
                ready: true,
                active: false,
            }],
            runtime_profiles: Vec::new(),
            engine_capabilities: Vec::new(),
        }
    }

    fn diagnostic_report(
        status: EnvironmentHealthStatus,
        repair_kind: Option<DiagnosticRepairKind>,
    ) -> EnvironmentDiagnosticReport {
        let findings = (status != EnvironmentHealthStatus::Healthy)
            .then(|| EnvironmentDiagnosticFinding {
                finding_id: "private-finding".to_owned(),
                code: "engine_missing".to_owned(),
                component: DiagnosticComponent::InferenceEngine,
                severity: DiagnosticSeverity::Error,
                title: "Engine".to_owned(),
                summary: "Missing".to_owned(),
                target_id: None,
                repair_kind,
                repair_summary: None,
            })
            .into_iter()
            .collect();
        EnvironmentDiagnosticReport {
            report_id: "private-report".to_owned(),
            generated_at_ms: 1,
            status,
            engine_install_state: EngineInstallState::NotInstalled,
            engine_runtime_state: EngineRuntimeState::Stopped,
            ready_model_count: 0,
            unhealthy_model_count: 0,
            configured_backend_count: 0,
            open_code_installed: false,
            open_code_integration_state: OpenCodeIntegrationState::NotConfigured,
            installed_external_agent_count: 0,
            configured_external_agent_count: 0,
            attention_external_agent_count: 0,
            warning_count: u32::from(status == EnvironmentHealthStatus::NeedsAttention),
            error_count: u32::from(status == EnvironmentHealthStatus::Error),
            omitted_finding_count: 0,
            findings,
        }
    }

    fn assert_state(
        spec: &AgentTaskSpec,
        report: &EnvironmentDiagnosticReport,
        expected: AgentTaskVerificationState,
    ) {
        assert_eq!(
            evaluate_tool_observation(
                spec,
                AgentTaskToolObservation::EnvironmentDiagnostics(report)
            )
            .expect("repair evidence")
            .verification_state,
            expected
        );
    }

    fn external_spec(kind: AgentTaskKind) -> AgentTaskSpec {
        AgentTaskSpec::new(
            kind,
            AgentTaskTarget::external_agent(ExternalAgentIntegrationId::PiCodingAgent),
            AgentTaskProviderMode::Local,
        )
        .expect("external spec")
    }

    fn external_status(
        integration_state: ExternalAgentIntegrationState,
        configured: bool,
        managed_installation: bool,
    ) -> AgentExternalIntegrationStatus {
        AgentExternalIntegrationStatus {
            integration_id: "pi-coding-agent".to_owned(),
            display_name: "Pi Coding Agent".to_owned(),
            installed: true,
            managed_installation,
            version: None,
            integration_state,
            configured_protocol: configured
                .then_some(ExternalAgentGatewayProtocol::OpenAiChatCompletions),
            warning_count: 0,
        }
    }
}
