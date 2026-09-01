use std::{future::Future, pin::Pin};

use hal100_protocol::{
    ENGINE_ADAPTER_CONTRACT_REVISION, EngineAdapterId, EngineInstallPlan, EngineRemovePlan,
    InferenceAccelerator, InferenceArchitecture, InferenceDeployment, InferenceEngineDescriptor,
    InferenceEngineKind, InferenceEngineManifest, InferenceEngineOwnership,
    InferenceEngineSupportStatus, InferenceEngineSupportUnit, InferenceModelFormat,
    InferencePlatform, InferenceProtocol, ManagedEngineStatus,
};

use crate::{AgentRuntimeCapacityProfile, EngineManagerError, LlamaCppManager};

pub type EngineOperationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ManagedEngineStatus, EngineManagerError>> + Send + 'a>>;

/// Rust-owned lifecycle boundary for a managed local inference engine.
///
/// The adapter normalizes identity, capabilities, status and controlled lifecycle operations.
/// It intentionally does not accept arbitrary executable paths, commands or environment values.
pub trait InferenceEngineAdapter: Send + Sync {
    fn manifest(&self) -> InferenceEngineManifest;
    fn descriptor(&self) -> InferenceEngineDescriptor {
        self.manifest().descriptor
    }
    fn capacity_profile(&self) -> AgentRuntimeCapacityProfile;
    fn status(&self) -> Result<ManagedEngineStatus, EngineManagerError>;
    fn plan_install(&self) -> Result<EngineInstallPlan, EngineManagerError>;
    fn apply_install<'a>(&'a self, plan_id: &'a str) -> EngineOperationFuture<'a>;
    fn discard_install_plan(&self, plan_id: &str) -> Result<bool, EngineManagerError>;
    fn plan_remove(&self) -> Result<EngineRemovePlan, EngineManagerError>;
    fn apply_remove<'a>(&'a self, plan_id: &'a str) -> EngineOperationFuture<'a>;
    fn discard_remove_plan(&self, plan_id: &str) -> Result<bool, EngineManagerError>;
    fn start_model<'a>(&'a self, model_id: &'a str) -> EngineOperationFuture<'a>;
    fn force_start_model<'a>(&'a self, model_id: &'a str) -> EngineOperationFuture<'a>;
    fn stop<'a>(&'a self) -> EngineOperationFuture<'a>;
    fn force_stop<'a>(&'a self) -> EngineOperationFuture<'a>;
}

impl InferenceEngineAdapter for LlamaCppManager {
    fn manifest(&self) -> InferenceEngineManifest {
        llama_cpp_manifest()
    }

    fn capacity_profile(&self) -> AgentRuntimeCapacityProfile {
        LlamaCppManager::capacity_profile(self)
    }

    fn status(&self) -> Result<ManagedEngineStatus, EngineManagerError> {
        LlamaCppManager::status(self)
    }

    fn plan_install(&self) -> Result<EngineInstallPlan, EngineManagerError> {
        LlamaCppManager::plan_install(self)
    }

    fn apply_install<'a>(&'a self, plan_id: &'a str) -> EngineOperationFuture<'a> {
        Box::pin(LlamaCppManager::apply_install(self, plan_id))
    }

    fn discard_install_plan(&self, plan_id: &str) -> Result<bool, EngineManagerError> {
        LlamaCppManager::discard_install_plan(self, plan_id)
    }

    fn plan_remove(&self) -> Result<EngineRemovePlan, EngineManagerError> {
        LlamaCppManager::plan_remove(self)
    }

    fn apply_remove<'a>(&'a self, plan_id: &'a str) -> EngineOperationFuture<'a> {
        Box::pin(LlamaCppManager::apply_remove(self, plan_id))
    }

    fn discard_remove_plan(&self, plan_id: &str) -> Result<bool, EngineManagerError> {
        LlamaCppManager::discard_remove_plan(self, plan_id)
    }

    fn start_model<'a>(&'a self, model_id: &'a str) -> EngineOperationFuture<'a> {
        Box::pin(LlamaCppManager::start_model(self, model_id))
    }

    fn force_start_model<'a>(&'a self, model_id: &'a str) -> EngineOperationFuture<'a> {
        Box::pin(LlamaCppManager::force_start_model(self, model_id))
    }

    fn stop<'a>(&'a self) -> EngineOperationFuture<'a> {
        Box::pin(LlamaCppManager::stop(self))
    }

    fn force_stop<'a>(&'a self) -> EngineOperationFuture<'a> {
        Box::pin(LlamaCppManager::force_stop(self))
    }
}

/// Return the static manifest for HAL100's built-in managed llama.cpp adapter without requiring a
/// live database, gateway or process. This is shared by reporting/promotion tooling so the
/// managed engine cannot drift from the manifest used by the runtime manager.
pub fn llama_cpp_manifest() -> InferenceEngineManifest {
    InferenceEngineManifest {
        adapter_id: EngineAdapterId {
            engine: InferenceEngineKind::LlamaCpp,
            variant: "hal100-managed-metal".to_owned(),
            contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
        },
        descriptor: InferenceEngineDescriptor {
            kind: InferenceEngineKind::LlamaCpp,
            display_name: "HAL100 托管 llama.cpp".to_owned(),
            ownership: InferenceEngineOwnership::Managed,
            deployment: InferenceDeployment::Local,
            protocols: vec![InferenceProtocol::OpenAi],
            // The current pinned binary adapter is intentionally still Apple Silicon-only.
            // Additional platform assets must be introduced as independently verified adapters.
            platforms: vec![InferencePlatform::MacOs],
            architectures: vec![InferenceArchitecture::Aarch64],
            accelerators: vec![InferenceAccelerator::Metal],
            model_formats: vec![InferenceModelFormat::Gguf],
            managed_lifecycle: true,
        },
        support_units: vec![InferenceEngineSupportUnit {
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            accelerator: InferenceAccelerator::Metal,
            deployment: InferenceDeployment::Local,
            status: InferenceEngineSupportStatus::Managed,
            evidence: Some(crate::support_evidence_for(
                InferenceEngineKind::LlamaCpp,
                Some(InferenceEngineSupportStatus::Managed),
            )),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llama_cpp_adapter_exposes_bounded_current_build_capabilities() {
        fn assert_adapter_is_object_safe(_: Option<&dyn InferenceEngineAdapter>) {}
        assert_adapter_is_object_safe(None);

        let kind = InferenceEngineKind::LlamaCpp;
        assert_eq!(kind.storage_key(), "llama.cpp");
    }
}
