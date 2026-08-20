use std::{cmp::Reverse, sync::Arc, time::SystemTime};

use hal100_core::BUILT_IN_AGENT_RUNTIME;
use hal100_protocol::{
    DiagnosticComponent, DiagnosticRepairKind, DiagnosticSeverity, EngineInstallState,
    EngineRuntimeState, EnvironmentDiagnosticFinding, EnvironmentDiagnosticReport,
    EnvironmentHealthStatus, LocalModelState, OpenCodeIntegrationState,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AGENT_MODEL_ID, Database, DatabaseError, EngineManagerError, GatewayState, LlamaCppManager,
    OpenCodeIntegrationError, OpenCodeManager,
};

const MAX_FINDINGS: usize = 64;
const INTERNAL_AGENT_BACKEND_ID: &str = BUILT_IN_AGENT_RUNTIME.runtime_id;
const INTERNAL_CLOUD_BACKEND_PREFIX: &str = "hal100-agent-cloud-";

#[derive(Debug, Error)]
pub enum EnvironmentDiagnosticError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Engine(#[from] EngineManagerError),
    #[error(transparent)]
    OpenCode(#[from] OpenCodeIntegrationError),
}

#[derive(Clone)]
pub struct EnvironmentDiagnostics {
    database: Arc<Database>,
    engine: Arc<LlamaCppManager>,
    open_code: Arc<OpenCodeManager>,
    gateway: GatewayState,
}

struct FindingDraft {
    code: &'static str,
    component: DiagnosticComponent,
    severity: DiagnosticSeverity,
    title: String,
    summary: String,
    target_id: Option<String>,
    repair_kind: Option<DiagnosticRepairKind>,
    repair_summary: Option<String>,
}

impl EnvironmentDiagnostics {
    pub fn new(
        database: Arc<Database>,
        engine: Arc<LlamaCppManager>,
        open_code: Arc<OpenCodeManager>,
        gateway: GatewayState,
    ) -> Self {
        Self {
            database,
            engine,
            open_code,
            gateway,
        }
    }

    pub fn run(&self) -> Result<EnvironmentDiagnosticReport, EnvironmentDiagnosticError> {
        self.database.refresh_local_model_states()?;
        let engine = self.engine.status()?;
        let models = self.database.local_models()?;
        let open_code = self.open_code.detect()?;
        let routing = self.gateway.routing_snapshot();
        let mut findings = Vec::new();

        match engine.install_state {
            EngineInstallState::NotInstalled => findings.push(FindingDraft {
                code: "engine_not_installed",
                component: DiagnosticComponent::InferenceEngine,
                severity: DiagnosticSeverity::Warning,
                title: "llama.cpp 尚未安装".to_owned(),
                summary: "HAL100 当前不能启动托管的本地 GGUF 模型。".to_owned(),
                target_id: Some("llama.cpp".to_owned()),
                repair_kind: Some(DiagnosticRepairKind::InstallLlamaCpp),
                repair_summary: Some(
                    "安装固定且经过 SHA-256 校验的官方 Apple Silicon 构建。".to_owned(),
                ),
            }),
            EngineInstallState::VerificationFailed => findings.push(FindingDraft {
                code: "engine_verification_failed",
                component: DiagnosticComponent::InferenceEngine,
                severity: DiagnosticSeverity::Error,
                title: "llama.cpp 安装未通过完整性检查".to_owned(),
                summary:
                    "为避免覆盖未知文件，HAL100 不会直接自动修复；需要先确认卸载，再重新安装。"
                        .to_owned(),
                target_id: Some("llama.cpp".to_owned()),
                repair_kind: None,
                repair_summary: None,
            }),
            EngineInstallState::Installed => {}
        }
        if engine.runtime_state == EngineRuntimeState::Error {
            findings.push(FindingDraft {
                code: "engine_runtime_error",
                component: DiagnosticComponent::InferenceEngine,
                severity: DiagnosticSeverity::Error,
                title: "llama.cpp 运行进程异常退出".to_owned(),
                summary: "运行时已停止并恢复此前路由；请检查模型状态后重新启动。".to_owned(),
                target_id: engine.active_model_id.clone(),
                repair_kind: None,
                repair_summary: None,
            });
        }

        let ready_model_count = models
            .iter()
            .filter(|model| model.state == LocalModelState::Ready)
            .count();
        let unhealthy_model_count = models.len().saturating_sub(ready_model_count);
        if models.is_empty() {
            findings.push(FindingDraft {
                code: "model_library_empty",
                component: DiagnosticComponent::ModelLibrary,
                severity: DiagnosticSeverity::Info,
                title: "模型库为空".to_owned(),
                summary: "HAL100 中还没有可供本地推理使用的模型。".to_owned(),
                target_id: None,
                repair_kind: None,
                repair_summary: None,
            });
        }
        for model in &models {
            match model.state {
                LocalModelState::Ready => {}
                LocalModelState::Missing => {
                    let protected = model.id == AGENT_MODEL_ID;
                    findings.push(FindingDraft {
                        code: if protected {
                            "agent_model_missing"
                        } else {
                            "model_file_missing"
                        },
                        component: DiagnosticComponent::ModelLibrary,
                        severity: if protected {
                            DiagnosticSeverity::Error
                        } else {
                            DiagnosticSeverity::Warning
                        },
                        title: format!("模型文件缺失：{}", model.display_name),
                        summary: if protected {
                            "这是HAL100 Agent运行依赖，不能只清理索引；需要后续受控重装流程。"
                                .to_owned()
                        } else {
                            "索引仍在，但源文件已经不存在。".to_owned()
                        },
                        target_id: Some(model.id.clone()),
                        repair_kind: (!protected).then_some(DiagnosticRepairKind::RemoveModelIndex),
                        repair_summary: (!protected)
                            .then(|| "只清理HAL100中的失效索引，不执行文件删除。".to_owned()),
                    });
                }
                LocalModelState::Changed => findings.push(FindingDraft {
                    code: "model_file_changed",
                    component: DiagnosticComponent::ModelLibrary,
                    severity: DiagnosticSeverity::Error,
                    title: format!("模型文件发生变化：{}", model.display_name),
                    summary: "文件快照与索引不一致；HAL100不会自动信任或覆盖该文件。".to_owned(),
                    target_id: Some(model.id.clone()),
                    repair_kind: None,
                    repair_summary: None,
                }),
                LocalModelState::VerificationFailed => findings.push(FindingDraft {
                    code: "model_verification_failed",
                    component: DiagnosticComponent::ModelLibrary,
                    severity: DiagnosticSeverity::Error,
                    title: format!("模型完整性校验失败：{}", model.display_name),
                    summary: "该模型不能启动；需要重新下载或重新导入可信文件。".to_owned(),
                    target_id: Some(model.id.clone()),
                    repair_kind: None,
                    repair_summary: None,
                }),
            }
        }

        let user_backend_ids = routing
            .backend_ids
            .iter()
            .filter(|backend_id| {
                backend_id.as_str() != INTERNAL_AGENT_BACKEND_ID
                    && !backend_id.starts_with(INTERNAL_CLOUD_BACKEND_PREFIX)
            })
            .collect::<Vec<_>>();
        let user_active_backend = routing
            .active_backend_id
            .as_deref()
            .is_some_and(|backend_id| {
                backend_id != INTERNAL_AGENT_BACKEND_ID
                    && !backend_id.starts_with(INTERNAL_CLOUD_BACKEND_PREFIX)
            });
        if !user_active_backend {
            findings.push(FindingDraft {
                code: "gateway_no_active_backend",
                component: DiagnosticComponent::Gateway,
                severity: DiagnosticSeverity::Warning,
                title: "Gateway 当前没有活动推理后端".to_owned(),
                summary: "客户端请求可以到达HAL100，但在启动模型或选择后端前无法完成推理。"
                    .to_owned(),
                target_id: None,
                repair_kind: None,
                repair_summary: None,
            });
        }
        for health in &routing.backend_health {
            if health.circuit_open
                && health.backend_id != INTERNAL_AGENT_BACKEND_ID
                && !health.backend_id.starts_with(INTERNAL_CLOUD_BACKEND_PREFIX)
            {
                findings.push(FindingDraft {
                    code: "backend_circuit_open",
                    component: DiagnosticComponent::Gateway,
                    severity: DiagnosticSeverity::Error,
                    title: format!("推理后端暂时熔断：{}", health.backend_id),
                    summary: format!(
                        "该后端连续出现{}次基础设施故障；HAL100将在冷却后按下一次请求惰性探测。",
                        health.consecutive_failures
                    ),
                    target_id: Some(health.backend_id.clone()),
                    repair_kind: None,
                    repair_summary: None,
                });
            }
        }

        if !open_code.installed {
            findings.push(FindingDraft {
                code: "opencode_not_installed",
                component: DiagnosticComponent::OpenCode,
                severity: DiagnosticSeverity::Info,
                title: "未检测到 OpenCode".to_owned(),
                summary: "OpenCode是首版适配客户端，但不是HAL100 Gateway运行的必需组件。"
                    .to_owned(),
                target_id: Some("opencode".to_owned()),
                repair_kind: None,
                repair_summary: None,
            });
        } else {
            match open_code.integration_state {
                OpenCodeIntegrationState::NotConfigured => findings.push(FindingDraft {
                    code: "opencode_not_configured",
                    component: DiagnosticComponent::OpenCode,
                    severity: DiagnosticSeverity::Warning,
                    title: "OpenCode 尚未接入 HAL100".to_owned(),
                    summary: "已检测到OpenCode，但全局配置中没有HAL100托管Provider。".to_owned(),
                    target_id: Some("opencode".to_owned()),
                    repair_kind: Some(DiagnosticRepairKind::ConfigureOpenCode),
                    repair_summary: Some(
                        "保留用户默认模型，并写入HAL100 Gateway Provider和独立凭据引用。"
                            .to_owned(),
                    ),
                }),
                OpenCodeIntegrationState::Conflict => findings.push(FindingDraft {
                    code: "opencode_provider_conflict",
                    component: DiagnosticComponent::OpenCode,
                    severity: DiagnosticSeverity::Error,
                    title: "OpenCode 存在非HAL100所有的同名Provider".to_owned(),
                    summary: "HAL100拒绝覆盖用户或其他工具创建的Provider，需要用户先处理命名冲突。"
                        .to_owned(),
                    target_id: Some("opencode".to_owned()),
                    repair_kind: None,
                    repair_summary: None,
                }),
                OpenCodeIntegrationState::ModifiedOutsideHal100 => findings.push(FindingDraft {
                    code: "opencode_modified_outside_hal100",
                    component: DiagnosticComponent::OpenCode,
                    severity: DiagnosticSeverity::Error,
                    title: "HAL100托管的OpenCode配置已在外部发生变化".to_owned(),
                    summary: "为保护用户配置，HAL100不会自动覆盖；需要检查差异后重新确认。"
                        .to_owned(),
                    target_id: Some("opencode".to_owned()),
                    repair_kind: None,
                    repair_summary: None,
                }),
                OpenCodeIntegrationState::Configured => {
                    if !open_code.warnings.is_empty() {
                        findings.push(FindingDraft {
                            code: "opencode_configuration_warning",
                            component: DiagnosticComponent::OpenCode,
                            severity: DiagnosticSeverity::Info,
                            title: "OpenCode配置存在提示".to_owned(),
                            summary: format!(
                                "检测到{}项版本或配置优先级提示；未自动修改。",
                                open_code.warnings.len()
                            ),
                            target_id: Some("opencode".to_owned()),
                            repair_kind: None,
                            repair_summary: None,
                        });
                    }
                }
            }
        }

        findings.sort_by_key(|finding| {
            (
                Reverse(severity_rank(finding.severity)),
                component_rank(finding.component),
                finding.code,
                finding.target_id.clone(),
            )
        });
        let warning_count = findings
            .iter()
            .filter(|finding| finding.severity == DiagnosticSeverity::Warning)
            .count();
        let error_count = findings
            .iter()
            .filter(|finding| finding.severity == DiagnosticSeverity::Error)
            .count();
        let omitted_finding_count = findings.len().saturating_sub(MAX_FINDINGS);
        findings.truncate(MAX_FINDINGS);
        let findings = findings
            .into_iter()
            .enumerate()
            .map(|(index, finding)| EnvironmentDiagnosticFinding {
                finding_id: format!("finding-{}", index + 1),
                code: finding.code.to_owned(),
                component: finding.component,
                severity: finding.severity,
                title: finding.title,
                summary: finding.summary,
                target_id: finding.target_id,
                repair_kind: finding.repair_kind,
                repair_summary: finding.repair_summary,
            })
            .collect();
        Ok(EnvironmentDiagnosticReport {
            report_id: format!("diagnostic-{}", Uuid::new_v4().simple()),
            generated_at_ms: now_ms(),
            status: if error_count > 0 {
                EnvironmentHealthStatus::Error
            } else if warning_count > 0 {
                EnvironmentHealthStatus::NeedsAttention
            } else {
                EnvironmentHealthStatus::Healthy
            },
            engine_install_state: engine.install_state,
            engine_runtime_state: engine.runtime_state,
            ready_model_count: saturating_u32(ready_model_count),
            unhealthy_model_count: saturating_u32(unhealthy_model_count),
            configured_backend_count: saturating_u32(user_backend_ids.len()),
            open_code_installed: open_code.installed,
            open_code_integration_state: open_code.integration_state,
            warning_count: saturating_u32(warning_count),
            error_count: saturating_u32(error_count),
            omitted_finding_count: saturating_u32(omitted_finding_count),
            findings,
        })
    }
}

fn severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Info => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Error => 2,
    }
}

fn component_rank(component: DiagnosticComponent) -> u8 {
    match component {
        DiagnosticComponent::Gateway => 0,
        DiagnosticComponent::InferenceEngine => 1,
        DiagnosticComponent::ModelLibrary => 2,
        DiagnosticComponent::OpenCode => 3,
    }
}

fn saturating_u32(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use hal100_protocol::{LocalModelSummary, ModelOwnership, ModelSource};

    use super::*;
    use crate::{CredentialRegistry, GatewayState, OpenCodePaths, UsageWriter};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hal100-environment-diagnostics-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&path).expect("create diagnostic test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(
        directory: &TestDirectory,
        open_code_paths: OpenCodePaths,
    ) -> (Arc<Database>, EnvironmentDiagnostics) {
        let database = Arc::new(Database::open(directory.0.join("hal100.sqlite")).expect("DB"));
        let credentials = CredentialRegistry::new(Vec::new());
        let usage = UsageWriter::start(database.clone());
        let gateway = GatewayState::new(None, credentials.clone(), usage).expect("Gateway");
        let engine = Arc::new(
            LlamaCppManager::new(
                database.clone(),
                gateway.clone(),
                directory.0.join("engines/llama.cpp"),
            )
            .expect("engine"),
        );
        let open_code = Arc::new(OpenCodeManager::with_gateway_base_url(
            database.clone(),
            credentials,
            open_code_paths,
            "http://127.0.0.1:10100/v1".to_owned(),
        ));
        let diagnostics = EnvironmentDiagnostics::new(database.clone(), engine, open_code, gateway);
        (database, diagnostics)
    }

    #[test]
    fn reports_only_bounded_safe_findings_and_marks_deterministic_repairs() {
        let directory = TestDirectory::new();
        let home = directory.0.join("home");
        let paths = OpenCodePaths::for_macos(&home, &directory.0);
        let (database, diagnostics) = fixture(&directory, paths);
        let missing_path = directory.0.join("external-missing.gguf");
        database
            .upsert_external_model(
                &LocalModelSummary {
                    id: "external-missing".to_owned(),
                    display_name: "External Missing".to_owned(),
                    format: "gguf".to_owned(),
                    quantization: Some("Q4_K_M".to_owned()),
                    source: ModelSource::LocalFile,
                    repository: None,
                    revision: None,
                    file_name: "external-missing.gguf".to_owned(),
                    ownership: ModelOwnership::External,
                    license: None,
                    state: LocalModelState::Missing,
                    path: missing_path.display().to_string(),
                    size_bytes: 1,
                },
                1,
                &[7_u8; 32],
                now_ms(),
            )
            .expect("index missing model");

        let report = diagnostics.run().expect("diagnostic report");
        assert_eq!(report.status, EnvironmentHealthStatus::NeedsAttention);
        assert_eq!(report.ready_model_count, 0);
        assert_eq!(report.unhealthy_model_count, 1);
        assert!(report.findings.len() <= MAX_FINDINGS);
        assert!(report.findings.iter().any(|finding| {
            finding.code == "engine_not_installed"
                && finding.repair_kind == Some(DiagnosticRepairKind::InstallLlamaCpp)
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "model_file_missing"
                && finding.repair_kind == Some(DiagnosticRepairKind::RemoveModelIndex)
        }));
        let serialized = serde_json::to_string(&report).expect("serialize report");
        assert!(!serialized.contains(&missing_path.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn installed_unconfigured_opencode_has_one_confirmed_repair_path() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let home = directory.0.join("home");
        let binary = directory.0.join("opencode");
        fs::write(&binary, b"#!/bin/sh\necho 1.18.11\n").expect("fake OpenCode");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).expect("executable");
        let mut paths = OpenCodePaths::for_macos(&home, &directory.0);
        paths.binary_candidates = vec![binary];
        let (_, diagnostics) = fixture(&directory, paths);

        let report = diagnostics.run().expect("diagnostic report");
        assert!(report.open_code_installed);
        assert!(report.findings.iter().any(|finding| {
            finding.code == "opencode_not_configured"
                && finding.repair_kind == Some(DiagnosticRepairKind::ConfigureOpenCode)
        }));
    }
}
