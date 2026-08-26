use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use hal100_core::{
    AgentTaskClarificationKind as CoreClarificationKind, AgentTaskClarificationResolution,
    AgentTaskClarificationSpec, AgentTaskPhase, AgentTaskProviderMode, AgentTaskSpec,
    AgentTaskState, ExternalAgentIntegrationId,
};
use hal100_protocol::{
    AGENT_TASK_CHECKPOINT_SCHEMA_VERSION, AgentClarification, AgentClarificationAnswerRequest,
    AgentClarificationChoice, AgentClarificationKind, AgentClarificationOption,
    AgentExternalAgentChoice, AgentTaskCheckpoint, AgentTaskCheckpointPhase,
    AgentTaskEvidenceSource, AgentTaskRecoveryScope, AgentTaskVerificationState,
};

use crate::agent_task_evidence::AgentTaskEvidence;

const CLARIFICATION_TTL_MS: i64 = 5 * 60 * 1_000;
const MAX_CLARIFICATION_ATTEMPTS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentTaskRuntimeError {
    StateUnavailable,
    InvalidTransition,
    TaskUnavailable,
    PlanMismatch,
    ClarificationUnavailable,
    ClarificationMismatch,
    ClarificationExpired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentTaskBeginDisposition {
    Started,
    ResumedClarification,
    ResumedBoundedReplan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AgentTaskClarificationDisposition {
    Clarifying(AgentClarification),
    Task(AgentTaskSpec),
    Cancelled,
}

#[derive(Debug, Clone)]
struct AgentTaskRecord {
    spec: AgentTaskSpec,
    state: AgentTaskState,
    pending_plan_id: Option<String>,
    verification_state: AgentTaskVerificationState,
    evidence_source: Option<AgentTaskEvidenceSource>,
    evidence_observation_count: u8,
    replan_attempt_count: u8,
    checkpoint_sequence_offset: u32,
    resumed_from_clarification: bool,
    updated_at_ms: i64,
}

#[derive(Debug, Clone)]
struct AgentClarificationRecord {
    spec: AgentTaskClarificationSpec,
    state: AgentTaskState,
    checkpoint_sequence: u32,
    attempt_count: u8,
    expires_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone)]
enum AgentTaskRuntimeRecord {
    Clarification(AgentClarificationRecord),
    Task(AgentTaskRecord),
}

#[derive(Clone, Default)]
pub(super) struct AgentTaskRuntime {
    current: Arc<Mutex<Option<AgentTaskRuntimeRecord>>>,
}

impl AgentTaskRuntime {
    pub(super) fn supersede_clarification(
        &self,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        let Some(AgentTaskRuntimeRecord::Clarification(record)) = current.as_mut() else {
            return Ok(());
        };
        if record.state.phase() == AgentTaskPhase::Clarifying {
            transition(&mut record.state, AgentTaskPhase::Cancelled)?;
            record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
            record.updated_at_ms = updated_at_ms;
        }
        Ok(())
    }

    pub(super) fn begin(
        &self,
        spec: AgentTaskSpec,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        let mut state = AgentTaskState::default();
        transition(&mut state, AgentTaskPhase::Inspecting)?;
        let record = AgentTaskRecord {
            spec,
            state,
            pending_plan_id: None,
            verification_state: AgentTaskVerificationState::NotStarted,
            evidence_source: None,
            evidence_observation_count: 0,
            replan_attempt_count: 0,
            checkpoint_sequence_offset: 0,
            resumed_from_clarification: false,
            updated_at_ms,
        };
        *self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)? =
            Some(AgentTaskRuntimeRecord::Task(record));
        Ok(())
    }

    pub(super) fn begin_clarification(
        &self,
        spec: AgentTaskClarificationSpec,
        updated_at_ms: i64,
    ) -> Result<AgentClarification, AgentTaskRuntimeError> {
        let mut state = AgentTaskState::default();
        transition(&mut state, AgentTaskPhase::Clarifying)?;
        let record = AgentClarificationRecord {
            spec,
            state,
            checkpoint_sequence: 1,
            attempt_count: 0,
            expires_at_ms: updated_at_ms.saturating_add(CLARIFICATION_TTL_MS),
            updated_at_ms,
        };
        let presentation = clarification_presentation(&record);
        *self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)? =
            Some(AgentTaskRuntimeRecord::Clarification(record));
        Ok(presentation)
    }

    pub(super) fn resolve_clarification(
        &self,
        request: &AgentClarificationAnswerRequest,
        provider_mode: AgentTaskProviderMode,
        updated_at_ms: i64,
    ) -> Result<AgentTaskClarificationDisposition, AgentTaskRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        let Some(AgentTaskRuntimeRecord::Clarification(record)) = current.as_mut() else {
            return Err(AgentTaskRuntimeError::ClarificationUnavailable);
        };
        if record.state.phase() != AgentTaskPhase::Clarifying {
            return Err(AgentTaskRuntimeError::ClarificationUnavailable);
        }
        if updated_at_ms > record.expires_at_ms {
            transition(&mut record.state, AgentTaskPhase::Blocked)?;
            record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
            record.updated_at_ms = updated_at_ms;
            return Err(AgentTaskRuntimeError::ClarificationExpired);
        }
        if record.spec.provider_mode() != provider_mode
            || request.kind != clarification_kind(record.spec.kind())
        {
            consume_invalid_clarification(record, updated_at_ms)?;
            return Err(AgentTaskRuntimeError::ClarificationMismatch);
        }
        if request.choice == AgentClarificationChoice::Cancel {
            if request.external_agent.is_some() {
                consume_invalid_clarification(record, updated_at_ms)?;
                return Err(AgentTaskRuntimeError::ClarificationMismatch);
            }
            transition(&mut record.state, AgentTaskPhase::Cancelled)?;
            record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
            record.updated_at_ms = updated_at_ms;
            return Ok(AgentTaskClarificationDisposition::Cancelled);
        }

        let next_attempt_count = record.attempt_count.saturating_add(1);
        if next_attempt_count > MAX_CLARIFICATION_ATTEMPTS {
            transition(&mut record.state, AgentTaskPhase::Blocked)?;
            record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
            record.updated_at_ms = updated_at_ms;
            return Err(AgentTaskRuntimeError::ClarificationMismatch);
        }
        let resolution = match request.choice {
            AgentClarificationChoice::SelectExternalAgent => {
                let target = request
                    .external_agent
                    .map(external_agent_id)
                    .ok_or(AgentTaskRuntimeError::ClarificationMismatch);
                match target.and_then(|target| {
                    record
                        .spec
                        .select_external_agent(target)
                        .map_err(|_| AgentTaskRuntimeError::ClarificationMismatch)
                }) {
                    Ok(resolution) => resolution,
                    Err(error) => {
                        consume_invalid_clarification(record, updated_at_ms)?;
                        return Err(error);
                    }
                }
            }
            AgentClarificationChoice::RemoveManagedRuntime
            | AgentClarificationChoice::DisconnectOnly => {
                if request.external_agent.is_some() {
                    consume_invalid_clarification(record, updated_at_ms)?;
                    return Err(AgentTaskRuntimeError::ClarificationMismatch);
                }
                let remove_managed_runtime =
                    request.choice == AgentClarificationChoice::RemoveManagedRuntime;
                match record.spec.select_managed_ownership(remove_managed_runtime) {
                    Ok(spec) => AgentTaskClarificationResolution::Task(spec),
                    Err(_) => {
                        consume_invalid_clarification(record, updated_at_ms)?;
                        return Err(AgentTaskRuntimeError::ClarificationMismatch);
                    }
                }
            }
            AgentClarificationChoice::Cancel => unreachable!("cancel returns before resolution"),
        };
        record.attempt_count = next_attempt_count;
        record.updated_at_ms = updated_at_ms;
        match resolution {
            AgentTaskClarificationResolution::Clarify(spec) => {
                if record.attempt_count >= MAX_CLARIFICATION_ATTEMPTS {
                    transition(&mut record.state, AgentTaskPhase::Blocked)?;
                    record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
                    return Err(AgentTaskRuntimeError::ClarificationMismatch);
                }
                record.spec = spec;
                record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
                Ok(AgentTaskClarificationDisposition::Clarifying(
                    clarification_presentation(record),
                ))
            }
            AgentTaskClarificationResolution::Task(spec) => {
                transition(&mut record.state, AgentTaskPhase::Inspecting)?;
                let task_record = AgentTaskRecord {
                    spec: spec.clone(),
                    state: record.state,
                    pending_plan_id: None,
                    verification_state: AgentTaskVerificationState::NotStarted,
                    evidence_source: None,
                    evidence_observation_count: 0,
                    replan_attempt_count: 0,
                    checkpoint_sequence_offset: record.checkpoint_sequence.saturating_sub(1),
                    resumed_from_clarification: true,
                    updated_at_ms,
                };
                *current = Some(AgentTaskRuntimeRecord::Task(task_record));
                Ok(AgentTaskClarificationDisposition::Task(spec))
            }
        }
    }

    pub(super) fn begin_or_resume(
        &self,
        spec: AgentTaskSpec,
        updated_at_ms: i64,
    ) -> Result<AgentTaskBeginDisposition, AgentTaskRuntimeError> {
        {
            let mut current = self
                .current
                .lock()
                .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
            if let Some(AgentTaskRuntimeRecord::Task(record)) = current.as_mut()
                && record.spec == spec
                && record.state.phase() == AgentTaskPhase::Inspecting
                && record.resumed_from_clarification
            {
                record.resumed_from_clarification = false;
                record.updated_at_ms = updated_at_ms;
                return Ok(AgentTaskBeginDisposition::ResumedClarification);
            }
            if let Some(AgentTaskRuntimeRecord::Task(record)) = current.as_mut()
                && record.spec == spec
                && record.state.phase() == AgentTaskPhase::Planning
                && record.replan_attempt_count > 0
                && record.replan_attempt_count <= record.spec.constraints().max_replan_attempts
            {
                record.verification_state = AgentTaskVerificationState::Pending;
                record.evidence_source = None;
                record.evidence_observation_count = 0;
                record.updated_at_ms = updated_at_ms;
                return Ok(AgentTaskBeginDisposition::ResumedBoundedReplan);
            }
        }
        self.begin(spec, updated_at_ms)?;
        Ok(AgentTaskBeginDisposition::Started)
    }

    pub(super) fn enter_planning(&self, updated_at_ms: i64) -> Result<(), AgentTaskRuntimeError> {
        self.update(|record| {
            if !record.spec.constraints().requires_native_confirmation {
                return Err(AgentTaskRuntimeError::InvalidTransition);
            }
            transition(&mut record.state, AgentTaskPhase::Planning)?;
            record.verification_state = AgentTaskVerificationState::Pending;
            record.evidence_source = None;
            record.evidence_observation_count = 0;
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn complete_run(
        &self,
        pending_plan_id: Option<&str>,
        evidence: AgentTaskEvidence,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        self.update(|record| {
            if pending_plan_id.is_none() {
                validate_verification_evidence(record, evidence)?;
            }
            match record.state.phase() {
                AgentTaskPhase::Inspecting => {
                    if pending_plan_id.is_some() {
                        return Err(AgentTaskRuntimeError::InvalidTransition);
                    }
                    transition(&mut record.state, AgentTaskPhase::Verifying)?;
                    resolve_verification(record, evidence)?;
                }
                AgentTaskPhase::Planning => {
                    if let Some(plan_id) = pending_plan_id {
                        transition(&mut record.state, AgentTaskPhase::AwaitingConfirmation)?;
                        record.pending_plan_id = Some(plan_id.to_owned());
                        let pending = AgentTaskEvidence::pending_action_plan();
                        record.verification_state = pending.verification_state;
                        record.evidence_source = pending.source;
                        record.evidence_observation_count = pending.observation_count;
                    } else {
                        transition(&mut record.state, AgentTaskPhase::Verifying)?;
                        resolve_verification(record, evidence)?;
                    }
                }
                _ => return Err(AgentTaskRuntimeError::InvalidTransition),
            }
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn ensure_pending_plan(&self, plan_id: &str) -> Result<(), AgentTaskRuntimeError> {
        let current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        let record = current
            .as_ref()
            .and_then(|record| match record {
                AgentTaskRuntimeRecord::Task(record) => Some(record),
                AgentTaskRuntimeRecord::Clarification(_) => None,
            })
            .ok_or(AgentTaskRuntimeError::TaskUnavailable)?;
        if record.state.phase() != AgentTaskPhase::AwaitingConfirmation
            || record.pending_plan_id.as_deref() != Some(plan_id)
        {
            return Err(AgentTaskRuntimeError::PlanMismatch);
        }
        Ok(())
    }

    pub(super) fn start_execution(
        &self,
        plan_id: &str,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        self.update(|record| {
            if record.pending_plan_id.as_deref() != Some(plan_id) {
                return Err(AgentTaskRuntimeError::PlanMismatch);
            }
            transition(&mut record.state, AgentTaskPhase::Executing)?;
            record.pending_plan_id = None;
            record.verification_state = AgentTaskVerificationState::Pending;
            record.evidence_source = Some(AgentTaskEvidenceSource::ActionPlan);
            record.evidence_observation_count = 1;
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn begin_verification(
        &self,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        self.update(|record| {
            transition(&mut record.state, AgentTaskPhase::Verifying)?;
            record.verification_state = AgentTaskVerificationState::Pending;
            record.evidence_source = None;
            record.evidence_observation_count = 0;
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn complete_verification(
        &self,
        evidence: AgentTaskEvidence,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        self.update(|record| {
            if record.state.phase() != AgentTaskPhase::Verifying {
                return Err(AgentTaskRuntimeError::InvalidTransition);
            }
            resolve_verification(record, evidence)?;
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn fail(&self, updated_at_ms: i64) -> Result<(), AgentTaskRuntimeError> {
        self.update_if_present(|record| {
            if record.state.phase().is_terminal() {
                return Ok(());
            }
            transition(&mut record.state, AgentTaskPhase::Failed)?;
            record.pending_plan_id = None;
            record.verification_state = AgentTaskVerificationState::Failed;
            record.evidence_source = None;
            record.evidence_observation_count = 0;
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn cancel(&self, updated_at_ms: i64) -> Result<(), AgentTaskRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        let Some(record) = current.as_mut() else {
            return Ok(());
        };
        match record {
            AgentTaskRuntimeRecord::Clarification(record) => {
                if !record.state.phase().is_terminal() {
                    transition(&mut record.state, AgentTaskPhase::Cancelled)?;
                    record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
                    record.updated_at_ms = updated_at_ms;
                }
            }
            AgentTaskRuntimeRecord::Task(record) => {
                if !record.state.phase().is_terminal() {
                    transition(&mut record.state, AgentTaskPhase::Cancelled)?;
                    record.pending_plan_id = None;
                    record.updated_at_ms = updated_at_ms;
                }
            }
        }
        Ok(())
    }

    pub(super) fn cancel_plan(
        &self,
        plan_id: &str,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskRuntimeError> {
        self.update_if_present(|record| {
            if record.state.phase().is_terminal() {
                return Ok(());
            }
            if record.state.phase() != AgentTaskPhase::AwaitingConfirmation
                || record.pending_plan_id.as_deref() != Some(plan_id)
            {
                return Err(AgentTaskRuntimeError::PlanMismatch);
            }
            transition(&mut record.state, AgentTaskPhase::Cancelled)?;
            record.pending_plan_id = None;
            record.updated_at_ms = updated_at_ms;
            Ok(())
        })
    }

    pub(super) fn snapshot(&self) -> Result<Option<AgentTaskCheckpoint>, AgentTaskRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        if let Some(AgentTaskRuntimeRecord::Clarification(record)) = current.as_mut()
            && record.state.phase() == AgentTaskPhase::Clarifying
            && wall_clock_now_ms() > record.expires_at_ms
        {
            transition(&mut record.state, AgentTaskPhase::Blocked)?;
            record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
            record.updated_at_ms = wall_clock_now_ms();
        }
        Ok(current.as_ref().map(runtime_checkpoint))
    }

    pub(super) fn current_spec(&self) -> Result<AgentTaskSpec, AgentTaskRuntimeError> {
        let current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        current
            .as_ref()
            .and_then(|record| match record {
                AgentTaskRuntimeRecord::Task(record) => Some(record.spec.clone()),
                AgentTaskRuntimeRecord::Clarification(_) => None,
            })
            .ok_or(AgentTaskRuntimeError::TaskUnavailable)
    }

    fn update(
        &self,
        update: impl FnOnce(&mut AgentTaskRecord) -> Result<(), AgentTaskRuntimeError>,
    ) -> Result<(), AgentTaskRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        let Some(AgentTaskRuntimeRecord::Task(record)) = current.as_mut() else {
            return Err(AgentTaskRuntimeError::TaskUnavailable);
        };
        update(record)
    }

    fn update_if_present(
        &self,
        update: impl FnOnce(&mut AgentTaskRecord) -> Result<(), AgentTaskRuntimeError>,
    ) -> Result<(), AgentTaskRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskRuntimeError::StateUnavailable)?;
        let Some(AgentTaskRuntimeRecord::Task(record)) = current.as_mut() else {
            return Ok(());
        };
        update(record)
    }
}

fn resolve_verification(
    record: &mut AgentTaskRecord,
    evidence: AgentTaskEvidence,
) -> Result<(), AgentTaskRuntimeError> {
    validate_verification_evidence(record, evidence)?;
    record.verification_state = evidence.verification_state;
    record.evidence_source = evidence.source;
    record.evidence_observation_count = evidence.observation_count;
    match evidence.verification_state {
        AgentTaskVerificationState::Satisfied => {
            transition(&mut record.state, AgentTaskPhase::Completed)
        }
        AgentTaskVerificationState::Unsatisfied => {
            let max_replans = record.spec.constraints().max_replan_attempts;
            if record.spec.constraints().requires_native_confirmation
                && record.replan_attempt_count < max_replans
            {
                record.replan_attempt_count = record.replan_attempt_count.saturating_add(1);
                transition(&mut record.state, AgentTaskPhase::Planning)
            } else {
                transition(&mut record.state, AgentTaskPhase::Blocked)
            }
        }
        AgentTaskVerificationState::EvidenceUnavailable => {
            transition(&mut record.state, AgentTaskPhase::Blocked)
        }
        AgentTaskVerificationState::NotStarted
        | AgentTaskVerificationState::Pending
        | AgentTaskVerificationState::Failed => Err(AgentTaskRuntimeError::InvalidTransition),
    }
}

fn validate_verification_evidence(
    record: &AgentTaskRecord,
    evidence: AgentTaskEvidence,
) -> Result<(), AgentTaskRuntimeError> {
    if evidence
        .source
        .is_some_and(|source| !record.spec.accepts_evidence_source(source))
        || (!matches!(
            evidence.verification_state,
            AgentTaskVerificationState::EvidenceUnavailable
        ) && evidence.source.is_none())
    {
        return Err(AgentTaskRuntimeError::InvalidTransition);
    }
    match evidence.verification_state {
        AgentTaskVerificationState::Satisfied
        | AgentTaskVerificationState::Unsatisfied
        | AgentTaskVerificationState::EvidenceUnavailable => Ok(()),
        AgentTaskVerificationState::NotStarted
        | AgentTaskVerificationState::Pending
        | AgentTaskVerificationState::Failed => Err(AgentTaskRuntimeError::InvalidTransition),
    }
}

fn transition(
    state: &mut AgentTaskState,
    next: AgentTaskPhase,
) -> Result<(), AgentTaskRuntimeError> {
    state
        .transition(next)
        .map_err(|_| AgentTaskRuntimeError::InvalidTransition)
}

fn consume_invalid_clarification(
    record: &mut AgentClarificationRecord,
    updated_at_ms: i64,
) -> Result<(), AgentTaskRuntimeError> {
    record.attempt_count = record.attempt_count.saturating_add(1);
    record.checkpoint_sequence = record.checkpoint_sequence.saturating_add(1);
    record.updated_at_ms = updated_at_ms;
    if record.attempt_count >= MAX_CLARIFICATION_ATTEMPTS {
        transition(&mut record.state, AgentTaskPhase::Blocked)?;
    }
    Ok(())
}

fn clarification_presentation(record: &AgentClarificationRecord) -> AgentClarification {
    let options = match record.spec.kind() {
        CoreClarificationKind::ExternalAgentTarget
        | CoreClarificationKind::SingleMutationTarget => record
            .spec
            .external_agent_candidates()
            .iter()
            .copied()
            .map(|target| AgentClarificationOption {
                choice: AgentClarificationChoice::SelectExternalAgent,
                external_agent: Some(external_agent_choice(target)),
            })
            .chain(std::iter::once(AgentClarificationOption {
                choice: AgentClarificationChoice::Cancel,
                external_agent: None,
            }))
            .collect(),
        CoreClarificationKind::ManagedOwnership => {
            let mut options = Vec::with_capacity(3);
            if record.spec.external_agent_candidates()
                == [ExternalAgentIntegrationId::PiCodingAgent]
            {
                options.push(AgentClarificationOption {
                    choice: AgentClarificationChoice::RemoveManagedRuntime,
                    external_agent: None,
                });
            }
            options.push(AgentClarificationOption {
                choice: AgentClarificationChoice::DisconnectOnly,
                external_agent: None,
            });
            options.push(AgentClarificationOption {
                choice: AgentClarificationChoice::Cancel,
                external_agent: None,
            });
            options
        }
    };
    AgentClarification {
        kind: clarification_kind(record.spec.kind()),
        options,
        attempt_count: record.attempt_count,
        max_attempts: MAX_CLARIFICATION_ATTEMPTS,
        expires_at_ms: record.expires_at_ms,
    }
}

const fn clarification_kind(kind: CoreClarificationKind) -> AgentClarificationKind {
    match kind {
        CoreClarificationKind::ExternalAgentTarget => AgentClarificationKind::ExternalAgentTarget,
        CoreClarificationKind::ManagedOwnership => AgentClarificationKind::ManagedOwnership,
        CoreClarificationKind::SingleMutationTarget => AgentClarificationKind::SingleMutationTarget,
    }
}

const fn external_agent_choice(target: ExternalAgentIntegrationId) -> AgentExternalAgentChoice {
    match target {
        ExternalAgentIntegrationId::OpenCode => AgentExternalAgentChoice::OpenCode,
        ExternalAgentIntegrationId::PiCodingAgent => AgentExternalAgentChoice::PiCodingAgent,
        ExternalAgentIntegrationId::OpenClaw => AgentExternalAgentChoice::OpenClaw,
        ExternalAgentIntegrationId::HermesAgent => AgentExternalAgentChoice::HermesAgent,
    }
}

const fn external_agent_id(target: AgentExternalAgentChoice) -> ExternalAgentIntegrationId {
    match target {
        AgentExternalAgentChoice::OpenCode => ExternalAgentIntegrationId::OpenCode,
        AgentExternalAgentChoice::PiCodingAgent => ExternalAgentIntegrationId::PiCodingAgent,
        AgentExternalAgentChoice::OpenClaw => ExternalAgentIntegrationId::OpenClaw,
        AgentExternalAgentChoice::HermesAgent => ExternalAgentIntegrationId::HermesAgent,
    }
}

fn runtime_checkpoint(record: &AgentTaskRuntimeRecord) -> AgentTaskCheckpoint {
    match record {
        AgentTaskRuntimeRecord::Clarification(record) => clarification_checkpoint(record),
        AgentTaskRuntimeRecord::Task(record) => checkpoint(record),
    }
}

fn clarification_checkpoint(record: &AgentClarificationRecord) -> AgentTaskCheckpoint {
    let phase = checkpoint_phase(record.state.phase());
    AgentTaskCheckpoint {
        schema_version: AGENT_TASK_CHECKPOINT_SCHEMA_VERSION,
        phase,
        checkpoint_sequence: record.checkpoint_sequence,
        task_kind: record.spec.task_kind_key().to_owned(),
        target_kind: "external_agent".to_owned(),
        desired_state: record.spec.desired_state_key().to_owned(),
        provider_mode: record.spec.provider_mode().key().to_owned(),
        data_scope: record.spec.data_scope_key().to_owned(),
        success_predicate: record.spec.success_predicate_key().to_owned(),
        pending_action_plan: false,
        native_confirmation_required: true,
        verification_state: AgentTaskVerificationState::NotStarted,
        evidence_source: None,
        evidence_observation_count: 0,
        replan_attempt_count: 0,
        max_replan_attempts: 0,
        clarification_kind: Some(clarification_kind(record.spec.kind())),
        clarification_attempt_count: record.attempt_count,
        max_clarification_attempts: MAX_CLARIFICATION_ATTEMPTS,
        clarification_expires_at_ms: Some(record.expires_at_ms),
        recovery_scope: if phase == AgentTaskCheckpointPhase::Clarifying {
            AgentTaskRecoveryScope::InProcessClarification
        } else {
            AgentTaskRecoveryScope::None
        },
        updated_at_ms: record.updated_at_ms,
    }
}

fn wall_clock_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn checkpoint(record: &AgentTaskRecord) -> AgentTaskCheckpoint {
    let phase = checkpoint_phase(record.state.phase());
    let pending_action_plan =
        phase == AgentTaskCheckpointPhase::AwaitingConfirmation && record.pending_plan_id.is_some();
    AgentTaskCheckpoint {
        schema_version: AGENT_TASK_CHECKPOINT_SCHEMA_VERSION,
        phase,
        checkpoint_sequence: record
            .state
            .checkpoint_sequence()
            .saturating_add(record.checkpoint_sequence_offset),
        task_kind: record.spec.task_kind().key().to_owned(),
        target_kind: record.spec.target().kind().key().to_owned(),
        desired_state: record.spec.desired_state().key().to_owned(),
        provider_mode: record.spec.provider_mode().key().to_owned(),
        data_scope: record.spec.data_scope().key().to_owned(),
        success_predicate: record.spec.success_predicate().key().to_owned(),
        pending_action_plan,
        native_confirmation_required: record.spec.constraints().requires_native_confirmation,
        verification_state: record.verification_state,
        evidence_source: record.evidence_source,
        evidence_observation_count: record.evidence_observation_count,
        replan_attempt_count: record.replan_attempt_count,
        max_replan_attempts: record.spec.constraints().max_replan_attempts,
        clarification_kind: None,
        clarification_attempt_count: 0,
        max_clarification_attempts: 0,
        clarification_expires_at_ms: None,
        recovery_scope: if pending_action_plan {
            AgentTaskRecoveryScope::InProcessConfirmation
        } else {
            AgentTaskRecoveryScope::None
        },
        updated_at_ms: record.updated_at_ms,
    }
}

const fn checkpoint_phase(phase: AgentTaskPhase) -> AgentTaskCheckpointPhase {
    match phase {
        AgentTaskPhase::Draft => AgentTaskCheckpointPhase::Draft,
        AgentTaskPhase::Clarifying => AgentTaskCheckpointPhase::Clarifying,
        AgentTaskPhase::Inspecting => AgentTaskCheckpointPhase::Inspecting,
        AgentTaskPhase::Planning => AgentTaskCheckpointPhase::Planning,
        AgentTaskPhase::AwaitingConfirmation => AgentTaskCheckpointPhase::AwaitingConfirmation,
        AgentTaskPhase::Executing => AgentTaskCheckpointPhase::Executing,
        AgentTaskPhase::Verifying => AgentTaskCheckpointPhase::Verifying,
        AgentTaskPhase::Completed => AgentTaskCheckpointPhase::Completed,
        AgentTaskPhase::Blocked => AgentTaskCheckpointPhase::Blocked,
        AgentTaskPhase::Failed => AgentTaskCheckpointPhase::Failed,
        AgentTaskPhase::Cancelled => AgentTaskCheckpointPhase::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use hal100_core::{
        AgentTaskClarificationKind, AgentTaskIntentRouter, AgentTaskKind, AgentTaskProviderMode,
        AgentTaskSpec, AgentTaskTarget,
    };
    use serde_json::Value;

    use super::*;

    fn read_only_spec() -> AgentTaskSpec {
        AgentTaskSpec::new(
            AgentTaskKind::InspectSystem,
            AgentTaskTarget::system(),
            AgentTaskProviderMode::Local,
        )
        .expect("read-only task")
    }

    fn controlled_spec() -> AgentTaskSpec {
        AgentTaskSpec::new(
            AgentTaskKind::InstallEngine,
            AgentTaskTarget::llama_cpp(),
            AgentTaskProviderMode::Local,
        )
        .expect("controlled task")
    }

    fn satisfied_evidence() -> AgentTaskEvidence {
        AgentTaskEvidence::satisfied(AgentTaskEvidenceSource::SystemProbe)
    }

    fn controlled_satisfied_evidence() -> AgentTaskEvidence {
        AgentTaskEvidence::satisfied(AgentTaskEvidenceSource::EngineRecheck)
    }

    fn clarification_answer(
        kind: AgentClarificationKind,
        choice: AgentClarificationChoice,
        external_agent: Option<AgentExternalAgentChoice>,
    ) -> AgentClarificationAnswerRequest {
        AgentClarificationAnswerRequest {
            kind,
            choice,
            external_agent,
            cloud_target: None,
        }
    }

    #[test]
    fn external_target_clarification_continues_the_same_task_without_free_text() {
        let runtime = AgentTaskRuntime::default();
        let now = wall_clock_now_ms();
        let spec = AgentTaskIntentRouter::clarification_spec(
            "帮我把这个 Agent 配好",
            AgentTaskClarificationKind::ExternalAgentTarget,
            AgentTaskProviderMode::Local,
        )
        .expect("clarification spec");
        let clarification = runtime
            .begin_clarification(spec, now)
            .expect("begin clarification");
        assert_eq!(clarification.options.len(), 5);
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Clarifying);
        assert_eq!(
            checkpoint.recovery_scope,
            AgentTaskRecoveryScope::InProcessClarification
        );
        assert_eq!(checkpoint.clarification_attempt_count, 0);

        let AgentTaskClarificationDisposition::Task(task) = runtime
            .resolve_clarification(
                &clarification_answer(
                    AgentClarificationKind::ExternalAgentTarget,
                    AgentClarificationChoice::SelectExternalAgent,
                    Some(AgentExternalAgentChoice::OpenCode),
                ),
                AgentTaskProviderMode::Local,
                now + 1,
            )
            .expect("resolve target")
        else {
            panic!("target choice should complete the task spec");
        };
        assert_eq!(task.task_kind(), AgentTaskKind::ConfigureExternalAgent);
        assert_eq!(
            runtime
                .begin_or_resume(task, now + 2)
                .expect("resume clarified task"),
            AgentTaskBeginDisposition::ResumedClarification
        );
        runtime.enter_planning(now + 3).expect("planning");
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Planning);
        assert_eq!(checkpoint.checkpoint_sequence, 3);
        assert_eq!(checkpoint.clarification_kind, None);
    }

    #[test]
    fn multi_target_removal_uses_two_bounded_slots_then_resumes_once() {
        let runtime = AgentTaskRuntime::default();
        let now = wall_clock_now_ms();
        let spec = AgentTaskIntentRouter::clarification_spec(
            "卸载 Pi Coding Agent 和 OpenCode",
            AgentTaskClarificationKind::SingleMutationTarget,
            AgentTaskProviderMode::Local,
        )
        .expect("multi-target clarification");
        runtime
            .begin_clarification(spec, now)
            .expect("begin clarification");
        let AgentTaskClarificationDisposition::Clarifying(ownership) = runtime
            .resolve_clarification(
                &clarification_answer(
                    AgentClarificationKind::SingleMutationTarget,
                    AgentClarificationChoice::SelectExternalAgent,
                    Some(AgentExternalAgentChoice::PiCodingAgent),
                ),
                AgentTaskProviderMode::Local,
                now + 1,
            )
            .expect("select Pi")
        else {
            panic!("ownership should be the second bounded slot");
        };
        assert_eq!(ownership.kind, AgentClarificationKind::ManagedOwnership);
        assert_eq!(ownership.attempt_count, 1);
        let AgentTaskClarificationDisposition::Task(task) = runtime
            .resolve_clarification(
                &clarification_answer(
                    AgentClarificationKind::ManagedOwnership,
                    AgentClarificationChoice::RemoveManagedRuntime,
                    None,
                ),
                AgentTaskProviderMode::Local,
                now + 2,
            )
            .expect("select ownership")
        else {
            panic!("second slot should complete the task");
        };
        assert_eq!(task.task_kind(), AgentTaskKind::RemoveManagedExternalAgent);
        assert_eq!(
            runtime.begin_or_resume(task, now + 3).expect("resume task"),
            AgentTaskBeginDisposition::ResumedClarification
        );
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Inspecting);
        assert_eq!(checkpoint.checkpoint_sequence, 3);
    }

    #[test]
    fn mismatched_clarification_is_bounded_and_never_reopens_after_blocking() {
        let runtime = AgentTaskRuntime::default();
        let now = wall_clock_now_ms();
        let spec = AgentTaskIntentRouter::clarification_spec(
            "帮我把这个 Agent 配好",
            AgentTaskClarificationKind::ExternalAgentTarget,
            AgentTaskProviderMode::Local,
        )
        .expect("clarification spec");
        runtime
            .begin_clarification(spec, now)
            .expect("begin clarification");
        let invalid = clarification_answer(
            AgentClarificationKind::ManagedOwnership,
            AgentClarificationChoice::DisconnectOnly,
            None,
        );
        assert_eq!(
            runtime.resolve_clarification(&invalid, AgentTaskProviderMode::Local, now + 1),
            Err(AgentTaskRuntimeError::ClarificationMismatch)
        );
        assert_eq!(
            runtime.resolve_clarification(&invalid, AgentTaskProviderMode::Local, now + 2),
            Err(AgentTaskRuntimeError::ClarificationMismatch)
        );
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Blocked);
        assert_eq!(checkpoint.clarification_attempt_count, 2);
        assert_eq!(checkpoint.recovery_scope, AgentTaskRecoveryScope::None);
        assert_eq!(
            runtime.resolve_clarification(&invalid, AgentTaskProviderMode::Local, now + 3),
            Err(AgentTaskRuntimeError::ClarificationUnavailable)
        );
    }

    #[test]
    fn clarification_expiry_cancel_and_replacement_are_fail_closed() {
        let runtime = AgentTaskRuntime::default();
        let now = wall_clock_now_ms();
        let new_spec = || {
            AgentTaskIntentRouter::clarification_spec(
                "帮我把这个 Agent 配好",
                AgentTaskClarificationKind::ExternalAgentTarget,
                AgentTaskProviderMode::Local,
            )
            .expect("clarification spec")
        };
        runtime
            .begin_clarification(new_spec(), now)
            .expect("begin clarification");
        assert_eq!(
            runtime.resolve_clarification(
                &clarification_answer(
                    AgentClarificationKind::ExternalAgentTarget,
                    AgentClarificationChoice::SelectExternalAgent,
                    Some(AgentExternalAgentChoice::OpenCode),
                ),
                AgentTaskProviderMode::Local,
                now + CLARIFICATION_TTL_MS + 1,
            ),
            Err(AgentTaskRuntimeError::ClarificationExpired)
        );
        assert_eq!(
            runtime
                .snapshot()
                .expect("snapshot")
                .expect("checkpoint")
                .phase,
            AgentTaskCheckpointPhase::Blocked
        );

        runtime
            .begin_clarification(new_spec(), now + 10)
            .expect("replace blocked clarification");
        runtime
            .supersede_clarification(now + 11)
            .expect("supersede");
        assert_eq!(
            runtime
                .snapshot()
                .expect("snapshot")
                .expect("checkpoint")
                .phase,
            AgentTaskCheckpointPhase::Cancelled
        );
    }

    #[test]
    fn bounded_clarification_contract_matches_runtime_limits_and_redaction() {
        let manifest: Value = serde_json::from_str(include_str!(
            "../../../../contracts/agent-evals/v7-bounded-clarification.json"
        ))
        .expect("bounded clarification contract");
        assert_eq!(
            manifest["checkpointSchemaVersion"],
            u64::from(AGENT_TASK_CHECKPOINT_SCHEMA_VERSION)
        );
        assert_eq!(manifest["limits"]["ttlMs"], CLARIFICATION_TTL_MS);
        assert_eq!(
            manifest["limits"]["maxAttempts"],
            u64::from(MAX_CLARIFICATION_ATTEMPTS)
        );
        assert_eq!(manifest["scenarios"].as_array().map(Vec::len), Some(10));

        let runtime = AgentTaskRuntime::default();
        let now = wall_clock_now_ms();
        let spec = AgentTaskIntentRouter::clarification_spec(
            "帮我把这个 Agent 配好",
            AgentTaskClarificationKind::ExternalAgentTarget,
            AgentTaskProviderMode::Local,
        )
        .expect("clarification spec");
        runtime
            .begin_clarification(spec, now)
            .expect("begin clarification");
        let rendered = serde_json::to_value(runtime.snapshot().expect("snapshot"))
            .expect("checkpoint value")
            .to_string()
            .to_ascii_lowercase();
        for field in manifest["forbiddenCheckpointFields"]
            .as_array()
            .expect("forbidden fields")
        {
            let field = field.as_str().expect("field").to_ascii_lowercase();
            assert!(!rendered.contains(&field), "checkpoint leaked {field}");
        }
    }

    #[test]
    fn read_only_task_reaches_a_verified_terminal_checkpoint() {
        let runtime = AgentTaskRuntime::default();
        runtime.begin(read_only_spec(), 10).expect("begin");
        runtime
            .complete_run(None, satisfied_evidence(), 20)
            .expect("complete");
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Completed);
        assert_eq!(checkpoint.checkpoint_sequence, 3);
        assert_eq!(
            checkpoint.verification_state,
            AgentTaskVerificationState::Satisfied
        );
        assert!(!checkpoint.pending_action_plan);
        assert_eq!(checkpoint.recovery_scope, AgentTaskRecoveryScope::None);
    }

    #[test]
    fn controlled_task_resumes_only_at_the_exact_in_process_confirmation() {
        let runtime = AgentTaskRuntime::default();
        runtime.begin(controlled_spec(), 10).expect("begin");
        runtime.enter_planning(20).expect("planning");
        runtime
            .complete_run(Some("private-plan-1"), satisfied_evidence(), 30)
            .expect("await confirmation");
        assert_eq!(
            runtime.ensure_pending_plan("forged-plan"),
            Err(AgentTaskRuntimeError::PlanMismatch)
        );
        runtime
            .start_execution("private-plan-1", 40)
            .expect("execute exact plan");
        runtime.begin_verification(50).expect("verify");
        runtime
            .complete_verification(controlled_satisfied_evidence(), 60)
            .expect("complete");
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Completed);
        assert_eq!(checkpoint.checkpoint_sequence, 6);
        assert_eq!(checkpoint.recovery_scope, AgentTaskRecoveryScope::None);
    }

    #[test]
    fn checkpoint_serialization_contains_no_sensitive_or_authorizing_fields() {
        let runtime = AgentTaskRuntime::default();
        runtime.begin(controlled_spec(), 10).expect("begin");
        runtime.enter_planning(20).expect("planning");
        runtime
            .complete_run(Some("secret-plan-id"), satisfied_evidence(), 30)
            .expect("await confirmation");
        let value = serde_json::to_value(runtime.snapshot().expect("snapshot"))
            .expect("serialize checkpoint");
        assert_eq!(value["phase"], "awaitingConfirmation");
        assert_eq!(value["recoveryScope"], "inProcessConfirmation");
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "secret-plan-id",
            "planid",
            "runid",
            "targetid",
            "prompt",
            "answer",
            "path",
            "credential",
            "apikey",
            "toolresult",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "leaked {forbidden}: {rendered}"
            );
        }
    }

    #[test]
    fn evidence_from_an_unrelated_workflow_is_rejected_without_advancing_state() {
        let runtime = AgentTaskRuntime::default();
        runtime.begin(read_only_spec(), 10).expect("begin");
        assert_eq!(
            runtime.complete_run(
                None,
                AgentTaskEvidence::satisfied(AgentTaskEvidenceSource::EngineRecheck),
                20,
            ),
            Err(AgentTaskRuntimeError::InvalidTransition)
        );
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        assert_eq!(checkpoint.phase, AgentTaskCheckpointPhase::Inspecting);
        assert_eq!(
            checkpoint.verification_state,
            AgentTaskVerificationState::NotStarted
        );
    }

    #[test]
    fn task_checkpoint_v5_contract_matches_every_lifecycle_and_restart_rule() {
        let manifest: Value = serde_json::from_str(include_str!(
            "../../../../contracts/agent-evals/v5-task-checkpoints.json"
        ))
        .expect("task checkpoint evaluation manifest");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["thresholds"]["exactLifecycleRate"], 1);
        assert_eq!(manifest["thresholds"]["unauthorizedResumeCount"], 0);
        assert_eq!(manifest["thresholds"]["sensitiveCheckpointFieldCount"], 0);

        let scenarios = manifest["scenarios"]
            .as_array()
            .expect("checkpoint scenarios");
        assert_eq!(scenarios.len(), 10);
        for scenario in scenarios {
            let scenario_id = scenario["id"].as_str().expect("scenario id");
            let mut runtime = AgentTaskRuntime::default();
            let mut time = 0_i64;
            for event in scenario["events"].as_array().expect("scenario events") {
                time += 10;
                match event.as_str().expect("event name") {
                    "beginReadOnly" => runtime
                        .begin(read_only_spec(), time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "completeReadOnly" => runtime
                        .complete_run(None, satisfied_evidence(), time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "beginControlled" => {
                        runtime
                            .begin(controlled_spec(), time)
                            .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}"));
                        runtime
                            .enter_planning(time)
                            .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}"));
                    }
                    "planReady" => runtime
                        .complete_run(Some("private-plan-1"), satisfied_evidence(), time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "confirmExact" => runtime
                        .start_execution("private-plan-1", time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "verificationStarted" => runtime
                        .begin_verification(time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "verificationSatisfied" => runtime
                        .complete_verification(controlled_satisfied_evidence(), time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "cancelExact" | "expireExact" => runtime
                        .cancel_plan("private-plan-1", time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "confirmForged" => assert_eq!(
                        runtime.start_execution("forged-plan", time),
                        Err(AgentTaskRuntimeError::PlanMismatch),
                        "{scenario_id}"
                    ),
                    "supersede" => runtime
                        .cancel(time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "executionFailed" | "verificationFailed" => runtime
                        .fail(time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "restart" => runtime = AgentTaskRuntime::default(),
                    unknown => panic!("unknown event {unknown}: {scenario_id}"),
                }
            }

            let actual = serde_json::to_value(runtime.snapshot().expect("checkpoint snapshot"))
                .expect("serialize checkpoint");
            let expected = &scenario["expected"];
            if expected.is_null() {
                assert!(actual.is_null(), "{scenario_id}: {actual}");
                continue;
            }
            for field in [
                "phase",
                "checkpointSequence",
                "pendingActionPlan",
                "verificationState",
                "recoveryScope",
            ] {
                assert_eq!(actual[field], expected[field], "{scenario_id}: {field}");
            }
        }
    }

    #[test]
    fn success_predicate_v6_faults_fail_closed_and_replan_at_most_once() {
        let manifest: Value = serde_json::from_str(include_str!(
            "../../../../contracts/agent-evals/v6-success-predicates.json"
        ))
        .expect("success predicate manifest");
        assert_eq!(
            manifest["checkpointSchemaVersion"],
            u64::from(AGENT_TASK_CHECKPOINT_SCHEMA_VERSION)
        );
        assert_eq!(manifest["thresholds"]["maxReplanAttempts"], 1);

        let faults = manifest["faultInjections"]
            .as_array()
            .expect("fault scenarios");
        assert_eq!(faults.len(), 6);
        for scenario in faults {
            let scenario_id = scenario["id"].as_str().expect("scenario id");
            let runtime = AgentTaskRuntime::default();
            let mut time = 0_i64;
            for event in scenario["events"].as_array().expect("events") {
                time += 10;
                match event.as_str().expect("event") {
                    "beginReadOnly" => runtime
                        .begin(read_only_spec(), time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "modelClaimsSuccess" => {}
                    "beginControlled" => {
                        runtime
                            .begin(controlled_spec(), time)
                            .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}"));
                        runtime
                            .enter_planning(time)
                            .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}"));
                    }
                    "planReady" => runtime
                        .complete_run(Some("private-plan-1"), satisfied_evidence(), time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "confirmExact" => runtime
                        .start_execution("private-plan-1", time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "verificationStarted" => runtime
                        .begin_verification(time)
                        .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}")),
                    "evidenceUnsatisfied" => complete_fault_evidence(
                        &runtime,
                        fault_evidence(&runtime, false),
                        time,
                        scenario_id,
                    ),
                    "evidenceUnavailable" => complete_fault_evidence(
                        &runtime,
                        fault_evidence(&runtime, true),
                        time,
                        scenario_id,
                    ),
                    "resumeSameTask" => assert_eq!(
                        runtime.begin_or_resume(controlled_spec(), time),
                        Ok(AgentTaskBeginDisposition::ResumedBoundedReplan),
                        "{scenario_id}"
                    ),
                    unknown => panic!("unknown event {unknown}: {scenario_id}"),
                }
            }
            let actual =
                serde_json::to_value(runtime.snapshot().expect("snapshot").expect("checkpoint"))
                    .expect("serialize checkpoint");
            for field in ["phase", "verificationState", "replanAttemptCount"] {
                assert_eq!(
                    actual[field], scenario["expected"][field],
                    "{scenario_id}: {field}"
                );
            }
        }
    }

    fn complete_fault_evidence(
        runtime: &AgentTaskRuntime,
        evidence: AgentTaskEvidence,
        time: i64,
        scenario_id: &str,
    ) {
        let phase = runtime
            .snapshot()
            .expect("snapshot")
            .expect("checkpoint")
            .phase;
        if phase == AgentTaskCheckpointPhase::Inspecting {
            runtime
                .complete_run(None, evidence, time)
                .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}"));
        } else {
            runtime
                .complete_verification(evidence, time)
                .unwrap_or_else(|error| panic!("{scenario_id}: {error:?}"));
        }
    }

    fn fault_evidence(runtime: &AgentTaskRuntime, unavailable: bool) -> AgentTaskEvidence {
        let checkpoint = runtime.snapshot().expect("snapshot").expect("checkpoint");
        let source = if checkpoint.task_kind == "inspect_system" {
            AgentTaskEvidenceSource::SystemProbe
        } else {
            AgentTaskEvidenceSource::EngineRecheck
        };
        if unavailable {
            AgentTaskEvidence::unavailable(Some(source))
        } else {
            AgentTaskEvidence::unsatisfied(source)
        }
    }
}
