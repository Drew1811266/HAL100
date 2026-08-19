mod agent_runtime;
mod backend_discovery;
mod backend_manager;
mod database;
mod engine_manager;
mod environment_diagnostics;
mod error_aggregation;
mod gateway;
mod gateway_auth;
mod generic_client_manager;
mod jsonc_patch;
mod logging;
mod model_download;
mod model_import;
mod model_removal;
mod opencode_integration;
mod remote_model_catalog;
mod usage_writer;

pub use agent_runtime::{
    AGENT_IDLE_TIMEOUT, AGENT_MODEL_ALIAS, AGENT_MODEL_ID, AgentModelRuntime, AgentRuntimeError,
};
pub use backend_discovery::{BackendDiscoveryError, LocalBackendDiscoveryService};
pub use backend_manager::{BackendManager, BackendManagerError, BackendRestoreReport};
pub use database::{
    Database, DatabaseError, DownloadRecord, ManagedIntegrationRecord, StoredBackendRecord,
    StoredClientCredential, StoredModelRouteRecord, UsageRequestRecord,
};
pub use engine_manager::{EngineManagerError, LlamaCppManager};
pub use environment_diagnostics::{EnvironmentDiagnosticError, EnvironmentDiagnostics};
pub use error_aggregation::{ErrorEmission, RepeatedErrorAggregator};
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
pub use logging::{LoggingError, LoggingGuard, Redacted, init_structured_logging};
pub use model_download::{ModelDownloadError, ModelDownloadManager};
pub use model_import::{GgufImportError, GgufImportManager};
pub use model_removal::{ModelRemovalError, ModelRemovalManager};
pub use opencode_integration::{OpenCodeIntegrationError, OpenCodeManager, OpenCodePaths};
pub use remote_model_catalog::{RemoteModelCatalog, RemoteModelCatalogError};
pub use usage_writer::{UsageQueueError, UsageWriter};
