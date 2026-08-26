use std::sync::{Arc, Mutex};

use hal100_core::{ExternalAgentIntegrationId, ExternalAgentIntegrationRegistry};
use hal100_protocol::{AgentActionKind, AgentActionPlan, DiagnosticComponent};

const MAX_ACTION_PLAN_ID_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentActionPlanError {
    Unavailable,
    Expired,
}

#[derive(Debug, Clone)]
pub(super) struct PendingAgentAction {
    pub(super) plan: AgentActionPlan,
    pub(super) executor: AgentActionExecutor,
    pub(super) repair_verification: Option<AgentRepairVerification>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentRepairVerification {
    pub(super) code: String,
    pub(super) component: DiagnosticComponent,
    pub(super) target_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) enum AgentActionExecutor {
    StartOrSwitchModel {
        model_id: String,
    },
    StopModel {
        model_id: String,
    },
    DownloadModel {
        download_plan_id: String,
    },
    InstallLlamaCpp {
        engine_plan_id: String,
    },
    RemoveLlamaCpp {
        engine_plan_id: String,
    },
    RemoveModel {
        removal_plan_id: String,
        model_id: String,
    },
    InstallExternalAgent {
        integration_id: ExternalAgentIntegrationId,
        deployment_plan_id: String,
    },
    RemoveExternalAgent {
        integration_id: ExternalAgentIntegrationId,
        deployment_plan_id: String,
    },
    ConfigureExternalAgent {
        integration_id: ExternalAgentIntegrationId,
        integration_plan_id: String,
    },
    DisconnectExternalAgent {
        integration_id: ExternalAgentIntegrationId,
        integration_plan_id: String,
    },
}

impl AgentActionExecutor {
    const fn action_kind(&self) -> AgentActionKind {
        match self {
            Self::StartOrSwitchModel { .. } => AgentActionKind::StartOrSwitchModel,
            Self::StopModel { .. } => AgentActionKind::StopModel,
            Self::DownloadModel { .. } => AgentActionKind::DownloadModel,
            Self::InstallLlamaCpp { .. } => AgentActionKind::InstallLlamaCpp,
            Self::RemoveLlamaCpp { .. } => AgentActionKind::RemoveLlamaCpp,
            Self::RemoveModel { .. } => AgentActionKind::RemoveModel,
            Self::InstallExternalAgent { .. } => AgentActionKind::InstallExternalAgent,
            Self::RemoveExternalAgent { .. } => AgentActionKind::RemoveExternalAgent,
            Self::ConfigureExternalAgent { .. } => AgentActionKind::ConfigureExternalAgent,
            Self::DisconnectExternalAgent { .. } => AgentActionKind::DisconnectExternalAgent,
        }
    }

    fn matches_public_target(&self, target_id: &str) -> bool {
        match self {
            Self::StartOrSwitchModel { model_id }
            | Self::StopModel { model_id }
            | Self::RemoveModel { model_id, .. } => target_id == model_id,
            Self::InstallLlamaCpp { .. } | Self::RemoveLlamaCpp { .. } => target_id == "llama.cpp",
            Self::InstallExternalAgent { integration_id, .. }
            | Self::RemoveExternalAgent { integration_id, .. }
            | Self::ConfigureExternalAgent { integration_id, .. }
            | Self::DisconnectExternalAgent { integration_id, .. } => {
                target_id
                    == ExternalAgentIntegrationRegistry::descriptor(*integration_id).integration_id
            }
            // The public download target is a repository/revision/path tuple, while the executor
            // intentionally retains only the private one-use download-plan ID.
            Self::DownloadModel { .. } => !target_id.is_empty(),
        }
    }

    const fn supports_repair_verification(&self) -> bool {
        matches!(
            self,
            Self::InstallLlamaCpp { .. }
                | Self::ConfigureExternalAgent { .. }
                | Self::RemoveModel { .. }
        )
    }
}

impl PendingAgentAction {
    fn is_consistent(&self) -> bool {
        !self.plan.plan_id.is_empty()
            && self.plan.plan_id.chars().count() <= MAX_ACTION_PLAN_ID_CHARS
            && !self.plan.run_id.is_empty()
            && self.plan.requires_native_confirmation
            && self.plan.action_kind == self.executor.action_kind()
            && self.executor.matches_public_target(&self.plan.target_id)
            && self
                .repair_verification
                .as_ref()
                .is_none_or(|_| self.executor.supports_repair_verification())
    }
}

#[derive(Clone, Default)]
pub(super) struct AgentActionPlanStore {
    pending: Arc<Mutex<Option<PendingAgentAction>>>,
}

impl AgentActionPlanStore {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn current(
        &self,
        plan_id: &str,
        current_time_ms: i64,
    ) -> Result<AgentActionPlan, AgentActionPlanError> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| AgentActionPlanError::Unavailable)?;
        validate_plan(&pending, plan_id, current_time_ms)
    }

    pub(super) fn take(
        &self,
        plan_id: &str,
        current_time_ms: i64,
    ) -> Result<PendingAgentAction, AgentActionPlanError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| AgentActionPlanError::Unavailable)?;
        validate_plan(&pending, plan_id, current_time_ms)?;
        pending.take().ok_or(AgentActionPlanError::Unavailable)
    }

    pub(super) fn register(
        &self,
        pending_action: PendingAgentAction,
    ) -> Result<(), AgentActionPlanError> {
        if !pending_action.is_consistent() {
            return Err(AgentActionPlanError::Unavailable);
        }
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| AgentActionPlanError::Unavailable)?;
        if pending.is_some() {
            return Err(AgentActionPlanError::Unavailable);
        }
        *pending = Some(pending_action);
        Ok(())
    }

    pub(super) fn discard(&self, plan_id: &str) -> Option<PendingAgentAction> {
        self.pending.lock().ok().and_then(|mut pending| {
            pending
                .as_ref()
                .is_some_and(|pending| pending.plan.plan_id == plan_id)
                .then(|| pending.take())
                .flatten()
        })
    }

    pub(super) fn discard_any(&self) -> Option<PendingAgentAction> {
        self.pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.take())
    }

    pub(super) fn discard_expired(&self, current_time_ms: i64) -> Option<PendingAgentAction> {
        self.pending.lock().ok().and_then(|mut pending| {
            pending
                .as_ref()
                .is_some_and(|pending| current_time_ms > pending.plan.expires_at_ms)
                .then(|| pending.take())
                .flatten()
        })
    }
}

pub(super) const fn action_kind_key(kind: AgentActionKind) -> &'static str {
    match kind {
        AgentActionKind::StartOrSwitchModel => "start_or_switch_model",
        AgentActionKind::StopModel => "stop_model",
        AgentActionKind::DownloadModel => "download_model",
        AgentActionKind::InstallLlamaCpp => "install_llama_cpp",
        AgentActionKind::RemoveLlamaCpp => "remove_llama_cpp",
        AgentActionKind::RemoveModel => "remove_model",
        AgentActionKind::InstallExternalAgent => "install_external_agent",
        AgentActionKind::RemoveExternalAgent => "remove_external_agent",
        AgentActionKind::ConfigureExternalAgent => "configure_external_agent",
        AgentActionKind::DisconnectExternalAgent => "disconnect_external_agent",
    }
}

fn validate_plan(
    pending: &Option<PendingAgentAction>,
    plan_id: &str,
    current_time_ms: i64,
) -> Result<AgentActionPlan, AgentActionPlanError> {
    if plan_id.is_empty() || plan_id.chars().count() > MAX_ACTION_PLAN_ID_CHARS {
        return Err(AgentActionPlanError::Unavailable);
    }
    let plan = pending
        .as_ref()
        .map(|pending| &pending.plan)
        .filter(|plan| plan.plan_id == plan_id && plan.requires_native_confirmation)
        .cloned()
        .ok_or(AgentActionPlanError::Unavailable)?;
    if current_time_ms > plan.expires_at_ms {
        return Err(AgentActionPlanError::Expired);
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(expires_at_ms: i64) -> PendingAgentAction {
        PendingAgentAction {
            executor: AgentActionExecutor::StartOrSwitchModel {
                model_id: "managed-model-1".to_owned(),
            },
            repair_verification: None,
            plan: AgentActionPlan {
                plan_id: "agent-plan-1".to_owned(),
                run_id: "agent-run-1".to_owned(),
                action_kind: AgentActionKind::StartOrSwitchModel,
                target_id: "managed-model-1".to_owned(),
                target_name: "Qwen 测试模型".to_owned(),
                current_state: None,
                details: vec!["测试计划".to_owned()],
                expires_at_ms,
                action_summary: "安全启动模型".to_owned(),
                requires_native_confirmation: true,
            },
        }
    }

    #[test]
    fn exact_unexpired_plan_is_consumed_once_and_forgery_does_not_consume_it() {
        let store = AgentActionPlanStore::new();
        store.register(fixture(200)).expect("register plan");
        assert_eq!(
            store.take("forged-plan", 100).expect_err("forged plan"),
            AgentActionPlanError::Unavailable
        );
        let taken = store.take("agent-plan-1", 100).expect("take valid plan");
        assert_eq!(taken.plan.target_id, "managed-model-1");
        assert_eq!(
            store.take("agent-plan-1", 100).expect_err("one-use plan"),
            AgentActionPlanError::Unavailable
        );
    }

    #[test]
    fn expiry_and_confirmation_bypass_are_rejected_without_replacing_state() {
        let expired = AgentActionPlanStore::new();
        expired
            .register(fixture(99))
            .expect("register expired plan");
        assert_eq!(
            expired.take("agent-plan-1", 100).expect_err("expired plan"),
            AgentActionPlanError::Expired
        );
        assert!(expired.discard("agent-plan-1").is_some());

        let confirmation_bypass = AgentActionPlanStore::new();
        let mut bypass = fixture(200);
        bypass.plan.requires_native_confirmation = false;
        assert_eq!(
            confirmation_bypass.register(bypass),
            Err(AgentActionPlanError::Unavailable)
        );
        assert_eq!(
            confirmation_bypass
                .current(&"x".repeat(MAX_ACTION_PLAN_ID_CHARS + 1), 100)
                .expect_err("oversized id"),
            AgentActionPlanError::Unavailable
        );
    }

    #[test]
    fn pending_plan_cannot_be_silently_replaced_and_discard_requires_exact_id() {
        let store = AgentActionPlanStore::new();
        store.register(fixture(200)).expect("register first plan");
        assert_eq!(
            store.register(fixture(300)).expect_err("replace plan"),
            AgentActionPlanError::Unavailable
        );
        assert!(store.discard("forged-plan").is_none());
        assert!(store.current("agent-plan-1", 100).is_ok());
        assert!(store.discard("agent-plan-1").is_some());
        assert!(store.discard_any().is_none());
    }

    #[test]
    fn registration_rejects_public_plan_and_private_executor_mismatches() {
        let action_kind_mismatch = AgentActionPlanStore::new();
        let mut pending = fixture(200);
        pending.plan.action_kind = AgentActionKind::RemoveModel;
        assert_eq!(
            action_kind_mismatch.register(pending),
            Err(AgentActionPlanError::Unavailable)
        );

        let target_mismatch = AgentActionPlanStore::new();
        let mut pending = fixture(200);
        pending.plan.target_id = "different-model".to_owned();
        assert_eq!(
            target_mismatch.register(pending),
            Err(AgentActionPlanError::Unavailable)
        );

        let repair_mismatch = AgentActionPlanStore::new();
        let mut pending = fixture(200);
        pending.repair_verification = Some(AgentRepairVerification {
            code: "fixture".to_owned(),
            component: DiagnosticComponent::InferenceEngine,
            target_id: None,
        });
        assert_eq!(
            repair_mismatch.register(pending),
            Err(AgentActionPlanError::Unavailable)
        );
    }

    #[test]
    fn expiry_reconciliation_discards_only_after_the_fixed_deadline() {
        let store = AgentActionPlanStore::new();
        store.register(fixture(200)).expect("register plan");
        assert!(store.discard_expired(200).is_none());
        let expired = store.discard_expired(201).expect("expired plan");
        assert_eq!(expired.plan.plan_id, "agent-plan-1");
        assert!(store.discard_expired(300).is_none());
    }
}
