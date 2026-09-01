mod agent_runtime;
mod backend_discovery;
mod backend_manager;
mod database;
mod engine_acceptance_evidence;
mod engine_http_boundary;
mod engine_manager;
mod engine_observation;
mod engine_recommendation;
mod engine_support_evidence;
mod engine_support_report;
mod engine_target;
mod environment_diagnostics;
mod error_aggregation;
mod external_inference_engine_adapter;
mod external_integration_control;
mod gateway;
mod gateway_auth;
mod generic_client_manager;
mod hermes_agent_integration;
mod hermes_config;
mod inference_engine_adapter;
mod inference_engine_registry;
mod jsonc_patch;
mod lmdeploy_external_adapter;
mod logging;
mod managed_deployment;
mod managed_file;
mod mlc_llm_external_adapter;
mod mlx_lm_external_adapter;
mod model_capacity;
mod model_download;
mod model_import;
mod model_removal;
mod openai_protocol_qualification;
mod openclaw_integration;
mod opencode_integration;
mod openvino_external_adapter;
mod pi_coding_agent_integration;
mod remote_model_catalog;
mod runtime_activation_journal;
mod runtime_profile_manager;
mod runtime_profile_repository;
mod sglang_external_adapter;
mod tensorrt_llm_external_adapter;
mod usage_writer;
mod vllm_external_adapter;

pub use agent_runtime::{
    AGENT_IDLE_TIMEOUT, AGENT_MODEL_ALIAS, AGENT_MODEL_ID, AgentModelRuntime, AgentRuntimeError,
};
pub use backend_discovery::{BackendDiscoveryError, LocalBackendDiscoveryService};
pub use backend_manager::{BackendManager, BackendManagerError, BackendRestoreReport};
pub use database::{
    Database, DatabaseError, DownloadRecord, ManagedIntegrationRecord,
    ManagedIntegrationResourceRecord, ManagedIntegrationResourceRole, RuntimeActivationPhase,
    StoredActiveGatewayRoute, StoredBackendEngineBinding, StoredBackendRecord,
    StoredClientCredential, StoredModelRouteRecord, StoredRuntimeActivationJournal,
    StoredRuntimeProfileRecord, StoredRuntimeProfileVerification, UsageRequestRecord,
};
pub use engine_acceptance_evidence::{
    INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
    INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION,
    INFERENCE_ENGINE_NATIVE_HOST_ATTESTATION_REVISION, InferenceEngineAcceptanceEvidence,
    InferenceEngineAcceptanceEvidenceError, InferenceEngineAcceptanceHostAttestation,
    InferenceEngineAcceptanceHostAttestationKind, InferenceEngineAcceptanceLedger,
    InferenceEngineAcceptanceModelEvidence, InferenceEngineAcceptanceRecord,
    InferenceEngineAcceptanceResilience, InferenceEngineAcceptanceRun,
    InferenceEngineAcceptanceRunOutcome, InferenceEngineAcceptanceStability,
    write_acceptance_run_exclusive,
};
pub use engine_http_boundary::{BoundedEngineHttpClient, EngineHttpError};
pub use engine_manager::{EngineManagerError, LlamaCppManager};
pub use engine_observation::{
    DEFAULT_ENGINE_DISPLAY_CACHE_TTL, EngineObservation, EngineObservationPurpose,
    EngineObservationService,
};
pub use engine_recommendation::recommendation_for;
pub use engine_support_evidence::support_evidence_for;
pub use engine_support_report::{
    INFERENCE_ENGINE_SUPPORT_REPORT_SCHEMA_VERSION, InferenceEngineAdapterSupportCoverage,
    InferenceEngineReviewedPerformanceProfile, InferenceEngineSupportCellCoverage,
    InferenceEngineSupportCoverageReport, InferenceEngineSupportReportError,
    build_support_coverage_report, build_support_coverage_report_with_protocol_capability_hashes,
};
pub use engine_target::{
    EngineInstance, EngineInstanceId, EngineRequestAuth, EngineTargetError, EngineTargetKey,
    ValidatedEngineOrigin, VerifiedEngineTarget,
};
pub use environment_diagnostics::{EnvironmentDiagnosticError, EnvironmentDiagnostics};
pub use error_aggregation::{ErrorEmission, RepeatedErrorAggregator};
pub use external_inference_engine_adapter::{
    EngineInspector, ExternalEngineAdapterError, ExternalEngineInspectionFuture,
    ExternalEngineQualificationFuture, ExternalInferenceEngineAdapter,
    ExternalInferenceEngineRegistry, OllamaExternalEngineAdapter,
    ollama_agent_protocol_capabilities, ollama_agent_protocol_capability_hash,
    protocol_capability_hash, protocol_capability_set,
};
pub use external_integration_control::{
    BoundedCommandError, BoundedCommandRunner, ExternalModelProfileRegistry, ModelProfileError,
    PendingPlanError, PendingPlanStore, PendingPlanTicket,
};
pub use gateway::{
    ActiveGatewayRoute, BackendAuthStyle, BackendConfig, BackendHealthSnapshot,
    DEFAULT_GATEWAY_ADDRESS, GatewayBuildError, GatewayProbeError, GatewayRouteError,
    GatewayRouteSwitchError, GatewayRoutingSnapshot, GatewayState, ModelRouteSnapshot,
    gateway_router, health_router, serve_gateway,
};
pub use gateway_auth::{
    ClientCredentialError, CredentialRegistry, hash_client_key, stored_client_credential,
};
pub use generic_client_manager::{GenericClientManager, GenericClientManagerError};
pub use hermes_agent_integration::{
    HermesAgentIntegrationAdapter, HermesAgentIntegrationError, HermesAgentPaths,
};
pub use inference_engine_adapter::{
    EngineOperationFuture, InferenceEngineAdapter, llama_cpp_manifest,
};
pub use inference_engine_registry::{
    InferenceEngineManifestRegistry, InferenceEngineRegistryError,
};
pub use lmdeploy_external_adapter::{
    LmDeployExternalEngineAdapter, lmdeploy_agent_protocol_capabilities,
    lmdeploy_agent_protocol_capability_hash,
};
pub use logging::{LoggingError, LoggingGuard, Redacted, init_structured_logging};
pub use managed_deployment::{
    ManagedExternalAgentDeploymentError, ManagedExternalAgentDeploymentManager,
    ManagedExternalAgentInstallPlan, ManagedExternalAgentInstallResult,
    ManagedExternalAgentRemovalPlan, ManagedExternalAgentRemovalResult,
};
pub use managed_file::{
    ManagedFileError, atomic_write as atomic_write_managed_file,
    backup_path as managed_backup_path, content_hash as managed_content_hash,
    existing_mode as managed_file_mode, read_bounded as read_managed_file,
    reject_symlink as reject_managed_file_symlink, sync_directory as sync_managed_directory,
    write_new_file as write_new_managed_file,
};
pub use mlc_llm_external_adapter::{
    MlcLlmExternalEngineAdapter, mlc_llm_agent_protocol_capabilities,
    mlc_llm_agent_protocol_capability_hash,
};
pub use mlx_lm_external_adapter::{
    MlxLmExternalEngineAdapter, mlx_lm_agent_protocol_capabilities,
    mlx_lm_agent_protocol_capability_hash,
};
pub use model_capacity::{
    AGENT_BASELINE_CONTEXT_WINDOW_TOKENS, AGENT_CAPACITY_PROFILE_REVISION, AGENT_MAX_OUTPUT_TOKENS,
    AGENT_PI_RESERVED_TOKENS, AGENT_STANDARD_CONTEXT_WINDOW_TOKENS,
    AGENT_STANDARD_MIN_UNIFIED_MEMORY_BYTES, AgentRuntimeCapacityProfile,
    MANAGED_ROUTE_MAX_OUTPUT_TOKENS, MANAGED_ROUTE_PROFILE_REVISION,
};
pub use model_download::{ModelDownloadError, ModelDownloadManager};
pub use model_import::{GgufImportError, GgufImportManager};
pub use model_removal::{ModelRemovalError, ModelRemovalManager};
pub use openai_protocol_qualification::{
    OPENAI_STABILITY_WORKLOAD_REVISION, OpenAiQualificationObservation, OpenAiQualificationOptions,
    OpenAiQualificationReasoningEffort, OpenAiStabilityObservation, OpenAiToolArgumentsMode,
    qualify_openai_agent_protocol, qualify_openai_runtime_stability,
};
pub use openclaw_integration::{
    OpenClawIntegrationAdapter, OpenClawIntegrationError, OpenClawPaths,
};
pub use opencode_integration::{
    OpenCodeIntegrationAdapter, OpenCodeIntegrationError, OpenCodeManager, OpenCodePaths,
};
pub use openvino_external_adapter::{
    OpenVinoExternalEngineAdapter, openvino_agent_protocol_capabilities,
    openvino_agent_protocol_capability_hash,
};
pub use pi_coding_agent_integration::{
    PiCodingAgentIntegrationAdapter, PiCodingAgentIntegrationError, PiCodingAgentPaths,
};
pub use remote_model_catalog::{RemoteModelCatalog, RemoteModelCatalogError};
pub use runtime_activation_journal::RuntimeActivationJournalRepository;
pub use runtime_profile_manager::{
    ENGINE_VERSION_NOT_EXPOSED, RuntimeProfileManager, RuntimeProfileManagerError,
};
pub use runtime_profile_repository::RuntimeProfileRepository;
pub use sglang_external_adapter::{
    SglangExternalEngineAdapter, sglang_agent_protocol_capabilities,
    sglang_agent_protocol_capability_hash,
};
pub use tensorrt_llm_external_adapter::{
    TensorRtLlmExternalEngineAdapter, tensorrt_llm_agent_protocol_capabilities,
    tensorrt_llm_agent_protocol_capability_hash,
};
pub use usage_writer::{UsageQueueError, UsageWriter};
pub use vllm_external_adapter::{
    VllmExternalEngineAdapter, vllm_agent_protocol_capabilities,
    vllm_agent_protocol_capability_hash,
};
