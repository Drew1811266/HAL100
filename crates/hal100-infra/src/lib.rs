mod agent_runtime;
mod backend_discovery;
mod backend_manager;
mod database;
mod engine_manager;
mod environment_diagnostics;
mod error_aggregation;
mod external_integration_control;
mod gateway;
mod gateway_auth;
mod generic_client_manager;
mod hermes_agent_integration;
mod hermes_config;
mod jsonc_patch;
mod logging;
mod managed_deployment;
mod managed_file;
mod model_capacity;
mod model_download;
mod model_import;
mod model_removal;
mod openclaw_integration;
mod opencode_integration;
mod pi_coding_agent_integration;
mod remote_model_catalog;
mod usage_writer;

pub use agent_runtime::{
    AGENT_IDLE_TIMEOUT, AGENT_MODEL_ALIAS, AGENT_MODEL_ID, AgentModelRuntime, AgentRuntimeError,
};
pub use backend_discovery::{BackendDiscoveryError, LocalBackendDiscoveryService};
pub use backend_manager::{BackendManager, BackendManagerError, BackendRestoreReport};
pub use database::{
    Database, DatabaseError, DownloadRecord, ManagedIntegrationRecord,
    ManagedIntegrationResourceRecord, ManagedIntegrationResourceRole, StoredBackendRecord,
    StoredClientCredential, StoredModelRouteRecord, UsageRequestRecord,
};
pub use engine_manager::{EngineManagerError, LlamaCppManager};
pub use environment_diagnostics::{EnvironmentDiagnosticError, EnvironmentDiagnostics};
pub use error_aggregation::{ErrorEmission, RepeatedErrorAggregator};
pub use external_integration_control::{
    BoundedCommandError, BoundedCommandRunner, ExternalModelProfileRegistry, ModelProfileError,
    PendingPlanError, PendingPlanStore, PendingPlanTicket,
};
pub use gateway::{
    BackendAuthStyle, BackendConfig, BackendHealthSnapshot, DEFAULT_GATEWAY_ADDRESS,
    GatewayBuildError, GatewayProbeError, GatewayRouteError, GatewayRouteSwitchError,
    GatewayRoutingSnapshot, GatewayState, ModelRouteSnapshot, gateway_router, health_router,
    serve_gateway,
};
pub use gateway_auth::{
    ClientCredentialError, CredentialRegistry, hash_client_key, stored_client_credential,
};
pub use generic_client_manager::{GenericClientManager, GenericClientManagerError};
pub use hermes_agent_integration::{
    HermesAgentIntegrationAdapter, HermesAgentIntegrationError, HermesAgentPaths,
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
pub use model_capacity::{
    AGENT_BASELINE_CONTEXT_WINDOW_TOKENS, AGENT_CAPACITY_PROFILE_REVISION, AGENT_MAX_OUTPUT_TOKENS,
    AGENT_PI_RESERVED_TOKENS, AGENT_STANDARD_CONTEXT_WINDOW_TOKENS,
    AGENT_STANDARD_MIN_UNIFIED_MEMORY_BYTES, AgentRuntimeCapacityProfile,
    MANAGED_ROUTE_MAX_OUTPUT_TOKENS, MANAGED_ROUTE_PROFILE_REVISION,
};
pub use model_download::{ModelDownloadError, ModelDownloadManager};
pub use model_import::{GgufImportError, GgufImportManager};
pub use model_removal::{ModelRemovalError, ModelRemovalManager};
pub use openclaw_integration::{
    OpenClawIntegrationAdapter, OpenClawIntegrationError, OpenClawPaths,
};
pub use opencode_integration::{
    OpenCodeIntegrationAdapter, OpenCodeIntegrationError, OpenCodeManager, OpenCodePaths,
};
pub use pi_coding_agent_integration::{
    PiCodingAgentIntegrationAdapter, PiCodingAgentIntegrationError, PiCodingAgentPaths,
};
pub use remote_model_catalog::{RemoteModelCatalog, RemoteModelCatalogError};
pub use usage_writer::{UsageQueueError, UsageWriter};
