use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use hal100_protocol::{
    EngineAdapterId, InferenceEngineKind, InferenceEngineManifest, InferenceEngineOwnership,
    InferenceEngineSupportStatus,
};
use thiserror::Error;

const MAX_ADAPTER_ID_COMPONENT_BYTES: usize = 64;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum InferenceEngineRegistryError {
    #[error("推理引擎manifest身份、能力或支持单元无效")]
    InvalidManifest,
    #[error("推理引擎适配器身份重复")]
    DuplicateAdapter,
}

/// Immutable registry of compile-time engine manifests.
///
/// The registry contains capability declarations only. It cannot hold endpoints, credentials,
/// process handles, observations, or activation authority.
#[derive(Clone)]
pub struct InferenceEngineManifestRegistry {
    manifests: Arc<HashMap<EngineAdapterId, InferenceEngineManifest>>,
}

impl InferenceEngineManifestRegistry {
    pub fn new(
        manifests: Vec<InferenceEngineManifest>,
    ) -> Result<Self, InferenceEngineRegistryError> {
        let mut by_id = HashMap::with_capacity(manifests.len());
        for manifest in manifests {
            validate_manifest(&manifest)?;
            if by_id
                .insert(manifest.adapter_id.clone(), manifest)
                .is_some()
            {
                return Err(InferenceEngineRegistryError::DuplicateAdapter);
            }
        }
        Ok(Self {
            manifests: Arc::new(by_id),
        })
    }

    pub fn manifest(&self, id: &EngineAdapterId) -> Option<InferenceEngineManifest> {
        self.manifests.get(id).cloned()
    }

    pub fn manifests(&self) -> Vec<InferenceEngineManifest> {
        let mut manifests = self.manifests.values().cloned().collect::<Vec<_>>();
        manifests.sort_by(|left, right| {
            left.adapter_id
                .engine
                .storage_key()
                .cmp(right.adapter_id.engine.storage_key())
                .then_with(|| left.adapter_id.variant.cmp(&right.adapter_id.variant))
                .then_with(|| {
                    left.adapter_id
                        .contract_revision
                        .cmp(&right.adapter_id.contract_revision)
                })
        });
        manifests
    }

    pub fn manifests_for_engine(
        &self,
        engine: InferenceEngineKind,
    ) -> Vec<InferenceEngineManifest> {
        self.manifests()
            .into_iter()
            .filter(|manifest| manifest.adapter_id.engine == engine)
            .collect()
    }
}

fn validate_manifest(
    manifest: &InferenceEngineManifest,
) -> Result<(), InferenceEngineRegistryError> {
    if manifest.adapter_id.engine != manifest.descriptor.kind
        || !valid_id_component(&manifest.adapter_id.variant)
        || !valid_id_component(&manifest.adapter_id.contract_revision)
        || manifest.support_units.is_empty()
    {
        return Err(InferenceEngineRegistryError::InvalidManifest);
    }
    let mut support_cells = HashSet::with_capacity(manifest.support_units.len());
    for unit in &manifest.support_units {
        if !support_cells.insert((
            unit.platform,
            unit.architecture,
            unit.accelerator,
            unit.deployment,
        )) {
            return Err(InferenceEngineRegistryError::InvalidManifest);
        }
        if unit.deployment != manifest.descriptor.deployment
            || !manifest.descriptor.platforms.contains(&unit.platform)
            || !manifest
                .descriptor
                .architectures
                .contains(&unit.architecture)
            || !manifest.descriptor.accelerators.contains(&unit.accelerator)
        {
            return Err(InferenceEngineRegistryError::InvalidManifest);
        }
        match unit.status {
            InferenceEngineSupportStatus::Managed
                if manifest.descriptor.ownership != InferenceEngineOwnership::Managed
                    || !manifest.descriptor.managed_lifecycle =>
            {
                return Err(InferenceEngineRegistryError::InvalidManifest);
            }
            InferenceEngineSupportStatus::VerifiedExternal
                if manifest.descriptor.ownership != InferenceEngineOwnership::External
                    || manifest.descriptor.managed_lifecycle =>
            {
                return Err(InferenceEngineRegistryError::InvalidManifest);
            }
            _ => {}
        }
        let expected_evidence =
            crate::support_evidence_for(manifest.descriptor.kind, Some(unit.status));
        if unit
            .evidence
            .as_ref()
            .is_some_and(|evidence| evidence != &expected_evidence)
        {
            return Err(InferenceEngineRegistryError::InvalidManifest);
        }
        if matches!(
            unit.status,
            InferenceEngineSupportStatus::VerifiedExternal | InferenceEngineSupportStatus::Managed
        ) && unit.evidence.as_ref() != Some(&expected_evidence)
        {
            return Err(InferenceEngineRegistryError::InvalidManifest);
        }
    }
    Ok(())
}

fn valid_id_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ADAPTER_ID_COMPONENT_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/'))
}

#[cfg(test)]
mod tests {
    use hal100_protocol::{
        InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
        InferenceEngineDescriptor, InferenceEngineSupportUnit, InferenceModelFormat,
        InferencePlatform, InferenceProtocol,
    };

    use super::*;

    fn manifest(
        variant: &str,
        ownership: InferenceEngineOwnership,
        status: InferenceEngineSupportStatus,
    ) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: variant.to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Ollama,
                display_name: "Ollama fixture".to_owned(),
                ownership,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Metal],
                model_formats: vec![InferenceModelFormat::Gguf],
                managed_lifecycle: ownership == InferenceEngineOwnership::Managed,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
                status,
                evidence: matches!(
                    status,
                    InferenceEngineSupportStatus::Managed
                        | InferenceEngineSupportStatus::VerifiedExternal
                )
                .then(|| crate::support_evidence_for(InferenceEngineKind::Ollama, Some(status))),
            }],
        }
    }

    #[test]
    fn registry_allows_variants_but_rejects_duplicate_adapter_identity() {
        let first = manifest(
            "official-loopback-api",
            InferenceEngineOwnership::External,
            InferenceEngineSupportStatus::VerifiedExternal,
        );
        let second = manifest(
            "hal100-managed",
            InferenceEngineOwnership::Managed,
            InferenceEngineSupportStatus::Managed,
        );
        let registry = InferenceEngineManifestRegistry::new(vec![first.clone(), second])
            .expect("variant registry");

        assert_eq!(
            registry
                .manifests_for_engine(InferenceEngineKind::Ollama)
                .len(),
            2
        );
        assert_eq!(registry.manifest(&first.adapter_id), Some(first.clone()));
        assert_eq!(
            InferenceEngineManifestRegistry::new(vec![first.clone(), first]).err(),
            Some(InferenceEngineRegistryError::DuplicateAdapter)
        );
    }

    #[test]
    fn registry_rejects_support_claims_that_do_not_match_ownership_or_descriptor() {
        let false_managed = manifest(
            "external-managed-claim",
            InferenceEngineOwnership::External,
            InferenceEngineSupportStatus::Managed,
        );
        assert_eq!(
            InferenceEngineManifestRegistry::new(vec![false_managed]).err(),
            Some(InferenceEngineRegistryError::InvalidManifest)
        );

        let mut wrong_platform = manifest(
            "wrong-platform",
            InferenceEngineOwnership::External,
            InferenceEngineSupportStatus::Reserved,
        );
        wrong_platform.support_units[0].platform = InferencePlatform::Linux;
        assert_eq!(
            InferenceEngineManifestRegistry::new(vec![wrong_platform]).err(),
            Some(InferenceEngineRegistryError::InvalidManifest)
        );
    }

    #[test]
    fn registry_requires_complete_evidence_for_formal_support_cells() {
        let mut missing = manifest(
            "missing-evidence",
            InferenceEngineOwnership::External,
            InferenceEngineSupportStatus::VerifiedExternal,
        );
        missing.support_units[0].evidence = None;
        assert_eq!(
            InferenceEngineManifestRegistry::new(vec![missing]).err(),
            Some(InferenceEngineRegistryError::InvalidManifest)
        );

        let mut incomplete = manifest(
            "incomplete-evidence",
            InferenceEngineOwnership::External,
            InferenceEngineSupportStatus::VerifiedExternal,
        );
        incomplete.support_units[0].evidence = Some(crate::support_evidence_for(
            InferenceEngineKind::Ollama,
            Some(InferenceEngineSupportStatus::Connected),
        ));
        assert_eq!(
            InferenceEngineManifestRegistry::new(vec![incomplete]).err(),
            Some(InferenceEngineRegistryError::InvalidManifest)
        );
    }

    #[test]
    fn registry_rejects_duplicate_platform_support_cells() {
        let mut duplicate = manifest(
            "duplicate-cell",
            InferenceEngineOwnership::External,
            InferenceEngineSupportStatus::Connected,
        );
        duplicate
            .support_units
            .push(duplicate.support_units[0].clone());
        assert_eq!(
            InferenceEngineManifestRegistry::new(vec![duplicate]).err(),
            Some(InferenceEngineRegistryError::InvalidManifest)
        );
    }
}
