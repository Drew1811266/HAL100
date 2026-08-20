use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use hal100_infra::{AGENT_MODEL_ALIAS, Database, DatabaseError, GatewayState, StoredBackendRecord};
use hal100_protocol::{
    AgentCloudRunPreview, AgentCloudSessionPreview, AgentCloudSessionStatus, AgentCloudTarget,
    AgentPromptRequest, AgentProviderProtocol, BackendKind,
};
use serde_json::json;
use uuid::Uuid;

pub(super) const AGENT_CLIENT_APP_ID: &str = "hal100-agent";
pub(super) const CLOUD_AGENT_CLIENT_APP_ID: &str = "hal100-agent-cloud";
pub(super) const CLOUD_AGENT_ROUTE_PREFIX: &str = "hal100-agent-cloud-";

#[derive(Debug)]
pub(super) enum AgentProviderError {
    InvalidCloudTarget,
    CloudBackendUnavailable,
    CloudBackendUnsupported,
    CloudCredentialMissing,
    CloudSessionAlreadyActive,
    NoActiveCloudSession,
    StateUnavailable,
    Database(DatabaseError),
}

impl AgentProviderError {
    pub(super) const fn code(&self) -> &'static str {
        match self {
            Self::InvalidCloudTarget => "invalid_cloud_target",
            Self::CloudBackendUnavailable => "cloud_backend_unavailable",
            Self::CloudBackendUnsupported => "cloud_backend_unsupported",
            Self::CloudCredentialMissing => "cloud_credential_missing",
            Self::CloudSessionAlreadyActive => "cloud_session_already_active",
            Self::NoActiveCloudSession => "no_active_cloud_session",
            Self::StateUnavailable => "kernel_unavailable",
            Self::Database(_) => "database_failed",
        }
    }
}

impl From<DatabaseError> for AgentProviderError {
    fn from(error: DatabaseError) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone)]
struct ActiveCloudSession {
    target: AgentCloudTarget,
    activated_at_ms: i64,
    last_error_code: Option<String>,
}

pub(super) struct ResolvedAgentProvider {
    pub(super) protocol: AgentProviderProtocol,
    pub(super) model_id: String,
    pub(super) model_name: String,
    pub(super) client_app_id: &'static str,
    pub(super) backend_id: Option<String>,
    pub(super) uses_local_runtime: bool,
    pub(super) session_bound: bool,
}

pub(super) struct AgentProviderService {
    database: Arc<Database>,
    gateway: GatewayState,
    cloud_session: Mutex<Option<ActiveCloudSession>>,
}

impl AgentProviderService {
    pub(super) fn new(database: Arc<Database>, gateway: GatewayState) -> Self {
        Self {
            database,
            gateway,
            cloud_session: Mutex::new(None),
        }
    }

    pub(super) fn preview_cloud_run(
        &self,
        target: &AgentCloudTarget,
        prompt_bytes: u32,
    ) -> Result<AgentCloudRunPreview, AgentProviderError> {
        let (record, kind, _) = self.resolve_cloud_target(target)?;
        Ok(AgentCloudRunPreview {
            backend_id: record.id,
            backend_name: record.display_name,
            backend_kind: kind,
            api_root: record.api_root,
            model: target.model.trim().to_owned(),
            prompt_bytes,
            sends_system_instructions: true,
            may_send_tool_results: true,
            sends_credentials_to_sidecar: false,
            sends_local_paths: false,
            confirmation_summary:
                "本次任务会把任务文字、HAL100 固定系统指令及本次任务需要的只读工具结果发送给所选云端后端；云端 API Key 与本地文件路径不会发送给 Agent Sidecar。"
                    .to_owned(),
        })
    }

    pub(super) fn preview_cloud_session(
        &self,
        target: &AgentCloudTarget,
    ) -> Result<AgentCloudSessionPreview, AgentProviderError> {
        let (record, kind, _) = self.resolve_cloud_target(target)?;
        Ok(AgentCloudSessionPreview {
            backend_id: record.id,
            backend_name: record.display_name,
            backend_kind: kind,
            api_root: record.api_root,
            model: target.model.clone(),
            sends_future_prompts: true,
            sends_system_instructions: true,
            may_send_tool_results: true,
            stores_conversation_history: false,
            sends_credentials_to_sidecar: false,
            sends_local_paths: false,
            confirmation_summary:
                "启用后，当前应用会话中后续每项 HAL100 Agent 任务的文字、固定系统指令及任务需要的只读工具结果会发送到所选云端后端；不会保存对话历史，不会把云端 API Key 或本地文件路径交给 Sidecar。明确退出或重启 HAL100 后恢复本地默认。"
                    .to_owned(),
        })
    }

    pub(super) fn cloud_session_status(
        &self,
    ) -> Result<AgentCloudSessionStatus, AgentProviderError> {
        let session = self
            .cloud_session
            .lock()
            .map_err(|_| AgentProviderError::StateUnavailable)?
            .clone();
        let Some(session) = session else {
            return Ok(inactive_cloud_session_status());
        };
        match self.resolve_cloud_target(&session.target) {
            Ok((record, kind, protocol)) => Ok(AgentCloudSessionStatus {
                active: true,
                available: true,
                backend_id: Some(record.id),
                backend_name: Some(record.display_name),
                backend_kind: Some(kind),
                api_root: Some(record.api_root),
                model: Some(session.target.model),
                provider_protocol: Some(protocol),
                activated_at_ms: Some(session.activated_at_ms),
                last_error_code: session.last_error_code,
            }),
            Err(error) => Ok(AgentCloudSessionStatus {
                active: true,
                available: false,
                backend_id: Some(session.target.backend_id),
                backend_name: None,
                backend_kind: None,
                api_root: None,
                model: Some(session.target.model),
                provider_protocol: None,
                activated_at_ms: Some(session.activated_at_ms),
                last_error_code: session
                    .last_error_code
                    .or_else(|| Some(error.code().to_owned())),
            }),
        }
    }

    pub(super) fn start_cloud_session(
        &self,
        target: AgentCloudTarget,
    ) -> Result<AgentCloudSessionStatus, AgentProviderError> {
        let (record, kind, protocol) = self.resolve_cloud_target(&target)?;
        let activated_at_ms = now_ms();
        let mut session = self
            .cloud_session
            .lock()
            .map_err(|_| AgentProviderError::StateUnavailable)?;
        if session.is_some() {
            return Err(AgentProviderError::CloudSessionAlreadyActive);
        }
        self.database.insert_audit_event(
            "agent_cloud_session_started",
            "agent_cloud_session",
            &record.id,
            &json!({
                "provider": "cloud_session",
                "backendId": &record.id,
                "model": &target.model,
            })
            .to_string(),
            activated_at_ms,
        )?;
        *session = Some(ActiveCloudSession {
            target: target.clone(),
            activated_at_ms,
            last_error_code: None,
        });
        Ok(AgentCloudSessionStatus {
            active: true,
            available: true,
            backend_id: Some(record.id),
            backend_name: Some(record.display_name),
            backend_kind: Some(kind),
            api_root: Some(record.api_root),
            model: Some(target.model),
            provider_protocol: Some(protocol),
            activated_at_ms: Some(activated_at_ms),
            last_error_code: None,
        })
    }

    pub(super) fn stop_cloud_session(&self) -> Result<AgentCloudSessionStatus, AgentProviderError> {
        let session = self
            .cloud_session
            .lock()
            .map_err(|_| AgentProviderError::StateUnavailable)?
            .take()
            .ok_or(AgentProviderError::NoActiveCloudSession)?;
        let _ = self.database.insert_audit_event(
            "agent_cloud_session_stopped",
            "agent_cloud_session",
            &session.target.backend_id,
            &json!({
                "provider": "cloud_session",
                "backendId": session.target.backend_id,
                "model": session.target.model,
            })
            .to_string(),
            now_ms(),
        );
        Ok(inactive_cloud_session_status())
    }

    pub(super) fn record_cloud_session_error(&self, error_code: &str) {
        if let Ok(mut session) = self.cloud_session.lock()
            && let Some(session) = session.as_mut()
        {
            session.last_error_code = Some(error_code.to_owned());
        }
    }

    pub(super) fn clear_cloud_session_error(&self) {
        if let Ok(mut session) = self.cloud_session.lock()
            && let Some(session) = session.as_mut()
        {
            session.last_error_code = None;
        }
    }

    pub(super) fn resolve_agent_provider(
        &self,
        request: &AgentPromptRequest,
    ) -> Result<ResolvedAgentProvider, AgentProviderError> {
        let (target, session_bound) = if let Some(target) = request.cloud_target.as_ref() {
            (Some(target.clone()), false)
        } else {
            (
                self.cloud_session
                    .lock()
                    .map_err(|_| AgentProviderError::StateUnavailable)?
                    .as_ref()
                    .map(|session| session.target.clone()),
                true,
            )
        };
        let Some(target) = target else {
            return Ok(ResolvedAgentProvider {
                protocol: AgentProviderProtocol::LocalOpenAi,
                model_id: AGENT_MODEL_ALIAS.to_owned(),
                model_name: "Qwen3.5-2B Q4_K_M".to_owned(),
                client_app_id: AGENT_CLIENT_APP_ID,
                backend_id: None,
                uses_local_runtime: true,
                session_bound: false,
            });
        };
        let (record, _, protocol) = match self.resolve_cloud_target(&target) {
            Ok(resolved) => resolved,
            Err(error) => {
                if session_bound {
                    self.record_cloud_session_error(error.code());
                }
                return Err(error);
            }
        };
        Ok(ResolvedAgentProvider {
            protocol,
            model_id: format!("{CLOUD_AGENT_ROUTE_PREFIX}{}", Uuid::new_v4().simple()),
            model_name: target.model,
            client_app_id: CLOUD_AGENT_CLIENT_APP_ID,
            backend_id: Some(record.id),
            uses_local_runtime: false,
            session_bound,
        })
    }

    fn resolve_cloud_target(
        &self,
        target: &AgentCloudTarget,
    ) -> Result<(StoredBackendRecord, BackendKind, AgentProviderProtocol), AgentProviderError> {
        let backend_id = target.backend_id.trim();
        let model = target.model.trim();
        if backend_id != target.backend_id
            || model != target.model
            || backend_id.is_empty()
            || backend_id.len() > 128
            || !backend_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || model.is_empty()
            || model.len() > 256
            || model.chars().any(char::is_control)
        {
            return Err(AgentProviderError::InvalidCloudTarget);
        }
        let record = self
            .database
            .backends()?
            .into_iter()
            .find(|record| record.id == backend_id && record.enabled)
            .ok_or(AgentProviderError::CloudBackendUnavailable)?;
        let (kind, protocol) = match record.kind.as_str() {
            "external_openai" => (
                BackendKind::ExternalOpenAi,
                AgentProviderProtocol::CloudOpenAi,
            ),
            "external_anthropic" => (
                BackendKind::ExternalAnthropic,
                AgentProviderProtocol::CloudAnthropic,
            ),
            _ => return Err(AgentProviderError::CloudBackendUnsupported),
        };
        if record.credential_id.as_deref().is_none_or(str::is_empty) {
            return Err(AgentProviderError::CloudCredentialMissing);
        }
        if !self
            .gateway
            .routing_snapshot()
            .backend_ids
            .iter()
            .any(|loaded_id| loaded_id == backend_id)
        {
            return Err(AgentProviderError::CloudBackendUnavailable);
        }
        Ok((record, kind, protocol))
    }
}

fn inactive_cloud_session_status() -> AgentCloudSessionStatus {
    AgentCloudSessionStatus {
        active: false,
        available: false,
        backend_id: None,
        backend_name: None,
        backend_kind: None,
        api_root: None,
        model: None,
        provider_protocol: None,
        activated_at_ms: None,
        last_error_code: None,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    use hal100_infra::{BackendConfig, CredentialRegistry, UsageWriter};

    use super::*;

    fn provider_fixture() -> (AgentProviderService, GatewayState, Arc<Database>, PathBuf) {
        let data_dir = env::temp_dir().join(format!(
            "hal100-agent-provider-test-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&data_dir).expect("create provider test directory");
        let database = Arc::new(
            Database::open(data_dir.join("hal100.sqlite")).expect("open provider test database"),
        );
        database
            .upsert_backend(&StoredBackendRecord {
                id: "cloud-provider".to_owned(),
                display_name: "测试云端后端".to_owned(),
                kind: "external_anthropic".to_owned(),
                api_root: "http://127.0.0.1:48991/v1/".to_owned(),
                auth_style: "anthropic_api_key".to_owned(),
                credential_id: Some("keychain-reference".to_owned()),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("store provider backend");
        let credentials = CredentialRegistry::new(Vec::new());
        let usage_writer = UsageWriter::start(database.clone());
        let gateway = GatewayState::new(None, credentials, usage_writer).expect("create gateway");
        gateway
            .upsert_routed_backend(
                BackendConfig::new(
                    "cloud-provider",
                    "http://127.0.0.1:48991/v1/",
                    Some("fixture-upstream-secret".to_owned()),
                )
                .expect("provider backend config"),
            )
            .expect("load provider backend");
        (
            AgentProviderService::new(database.clone(), gateway.clone()),
            gateway,
            database,
            data_dir,
        )
    }

    #[test]
    fn rejects_ambiguous_cloud_target_identifiers_before_catalog_lookup() {
        let (provider, gateway, database, data_dir) = provider_fixture();
        for target in [
            AgentCloudTarget {
                backend_id: " cloud-provider".to_owned(),
                model: "claude-test".to_owned(),
            },
            AgentCloudTarget {
                backend_id: "cloud/provider".to_owned(),
                model: "claude-test".to_owned(),
            },
            AgentCloudTarget {
                backend_id: "cloud-provider".to_owned(),
                model: "claude\ntest".to_owned(),
            },
        ] {
            assert!(matches!(
                provider.preview_cloud_session(&target),
                Err(AgentProviderError::InvalidCloudTarget)
            ));
        }
        drop(provider);
        drop(gateway);
        drop(database);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn session_is_memory_only_and_an_unavailable_target_never_falls_back_locally() {
        let (provider, gateway, database, data_dir) = provider_fixture();
        let target = AgentCloudTarget {
            backend_id: "cloud-provider".to_owned(),
            model: "claude-test".to_owned(),
        };
        assert!(!provider.cloud_session_status().expect("status").active);
        provider
            .start_cloud_session(target)
            .expect("start provider session");
        let session_provider = provider
            .resolve_agent_provider(&AgentPromptRequest {
                prompt: "说明 HAL100 后端".to_owned(),
                cloud_target: None,
            })
            .expect("session provider");
        assert!(session_provider.session_bound);
        assert!(!session_provider.uses_local_runtime);

        let restarted = AgentProviderService::new(database.clone(), gateway.clone());
        assert!(
            !restarted
                .cloud_session_status()
                .expect("fresh status")
                .active
        );
        assert!(
            restarted
                .resolve_agent_provider(&AgentPromptRequest {
                    prompt: "说明 HAL100 本地模型".to_owned(),
                    cloud_target: None,
                })
                .expect("fresh local provider")
                .uses_local_runtime
        );

        gateway
            .remove_routed_backend("cloud-provider")
            .expect("unload provider backend");
        let status = provider.cloud_session_status().expect("unavailable status");
        assert!(status.active);
        assert!(!status.available);
        assert!(matches!(
            provider.resolve_agent_provider(&AgentPromptRequest {
                prompt: "说明 HAL100 后端".to_owned(),
                cloud_target: None,
            }),
            Err(AgentProviderError::CloudBackendUnavailable)
        ));
        provider
            .stop_cloud_session()
            .expect("stop provider session");

        drop(restarted);
        drop(provider);
        drop(gateway);
        drop(database);
        let _ = fs::remove_dir_all(data_dir);
    }
}
