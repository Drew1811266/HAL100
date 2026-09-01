use std::{fmt, sync::Arc};

use hal100_protocol::{
    EngineAdapterId, InferenceDeployment, InferenceEngineManifest, InferenceEngineOwnership,
};
use reqwest::{RequestBuilder, Url, header::HeaderValue};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_INSTANCE_ID_BYTES: usize = 128;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EngineTargetError {
    #[error("推理引擎实例身份无效")]
    InvalidInstanceId,
    #[error("推理引擎origin无效或不符合本机安全策略")]
    InvalidOrigin,
    #[error("推理引擎目标与适配器manifest不匹配")]
    ManifestMismatch,
    #[error("推理引擎目标认证材料无效")]
    InvalidAuthentication,
}

/// Request-scoped authentication owned by the Rust target boundary.
///
/// The value is deliberately non-serializable and its `Debug` implementation is redacted. It is
/// excluded from `EngineTargetKey`: credential rotation changes access, not engine identity, and
/// every authorization observation still performs a fresh network request.
#[derive(Clone, Default, PartialEq, Eq)]
pub enum EngineRequestAuth {
    #[default]
    None,
    Bearer(Arc<str>),
    ApiKey(Arc<str>),
}

impl EngineRequestAuth {
    pub fn bearer(value: &str) -> Result<Self, EngineTargetError> {
        validate_auth_value(value)?;
        Ok(Self::Bearer(Arc::from(value)))
    }

    pub fn api_key(value: &str) -> Result<Self, EngineTargetError> {
        validate_auth_value(value)?;
        Ok(Self::ApiKey(Arc::from(value)))
    }

    pub(crate) fn authenticate(&self, request: RequestBuilder) -> RequestBuilder {
        match self {
            Self::None => request,
            Self::Bearer(value) => request.bearer_auth(value.as_ref()),
            Self::ApiKey(value) => request.header("x-api-key", value.as_ref()),
        }
    }
}

impl fmt::Debug for EngineRequestAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("EngineRequestAuth::None"),
            Self::Bearer(_) => formatter.write_str("EngineRequestAuth::Bearer([REDACTED])"),
            Self::ApiKey(_) => formatter.write_str("EngineRequestAuth::ApiKey([REDACTED])"),
        }
    }
}

fn validate_auth_value(value: &str) -> Result<(), EngineTargetError> {
    if value.is_empty() || value.len() > 16 * 1024 || HeaderValue::from_str(value).is_err() {
        return Err(EngineTargetError::InvalidAuthentication);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineInstanceId(Arc<str>);

impl EngineInstanceId {
    pub fn new(value: &str) -> Result<Self, EngineTargetError> {
        if value.is_empty()
            || value.len() > MAX_INSTANCE_ID_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':')
            })
        {
            return Err(EngineTargetError::InvalidInstanceId);
        }
        Ok(Self(Arc::from(value)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedEngineOrigin {
    api_root: Url,
    fingerprint: [u8; 32],
}

impl ValidatedEngineOrigin {
    pub fn local_loopback(api_root: &str) -> Result<Self, EngineTargetError> {
        let api_root = Url::parse(api_root).map_err(|_| EngineTargetError::InvalidOrigin)?;
        if api_root.scheme() != "http"
            || api_root.host_str() != Some("127.0.0.1")
            || api_root.port().is_none()
            || !api_root.username().is_empty()
            || api_root.password().is_some()
            || api_root.query().is_some()
            || api_root.fragment().is_some()
            || !api_root.path().starts_with('/')
            || !api_root.path().ends_with('/')
        {
            return Err(EngineTargetError::InvalidOrigin);
        }
        let fingerprint = Sha256::digest(api_root.as_str().as_bytes()).into();
        Ok(Self {
            api_root,
            fingerprint,
        })
    }

    pub fn api_root(&self) -> &Url {
        &self.api_root
    }

    pub fn endpoint(&self, absolute_path: &str) -> Result<Url, EngineTargetError> {
        if !absolute_path.starts_with('/')
            || absolute_path.contains('?')
            || absolute_path.contains('#')
        {
            return Err(EngineTargetError::InvalidOrigin);
        }
        let mut endpoint = self.api_root.clone();
        endpoint.set_path(absolute_path);
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        Ok(endpoint)
    }

    /// Stable, non-secret identity for the validated origin.
    ///
    /// Callers may persist this value to bind verification evidence to one endpoint without
    /// persisting credentials or accepting an unvalidated URL as execution authority.
    pub fn fingerprint_hex(&self) -> String {
        self.fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

impl fmt::Debug for ValidatedEngineOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedEngineOrigin")
            .field("scheme", &self.api_root.scheme())
            .field("loopback", &true)
            .field("port", &self.api_root.port())
            .field("path", &self.api_root.path())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineTargetKey {
    instance_id: EngineInstanceId,
    adapter_id: EngineAdapterId,
    origin_fingerprint: [u8; 32],
    config_revision: u64,
}

impl EngineTargetKey {
    pub fn instance_id(&self) -> &EngineInstanceId {
        &self.instance_id
    }

    pub fn adapter_id(&self) -> &EngineAdapterId {
        &self.adapter_id
    }

    pub fn config_revision(&self) -> u64 {
        self.config_revision
    }
}

/// Rust-owned configuration identity for one concrete engine service.
///
/// This type is intentionally not serializable. Persisted backend records must be validated into
/// an instance again whenever their configuration revision changes.
#[derive(Clone, PartialEq, Eq)]
pub struct EngineInstance {
    id: EngineInstanceId,
    adapter_id: EngineAdapterId,
    origin: ValidatedEngineOrigin,
    config_revision: u64,
    request_auth: EngineRequestAuth,
}

impl EngineInstance {
    pub fn external_local(
        instance_id: &str,
        manifest: &InferenceEngineManifest,
        api_root: &str,
        config_revision: u64,
    ) -> Result<Self, EngineTargetError> {
        Self::external_local_with_auth(
            instance_id,
            manifest,
            api_root,
            config_revision,
            EngineRequestAuth::None,
        )
    }

    pub fn external_local_with_auth(
        instance_id: &str,
        manifest: &InferenceEngineManifest,
        api_root: &str,
        config_revision: u64,
        request_auth: EngineRequestAuth,
    ) -> Result<Self, EngineTargetError> {
        if manifest.adapter_id.engine != manifest.descriptor.kind
            || manifest.descriptor.ownership != InferenceEngineOwnership::External
            || manifest.descriptor.deployment != InferenceDeployment::Local
            || manifest.descriptor.managed_lifecycle
        {
            return Err(EngineTargetError::ManifestMismatch);
        }
        Ok(Self {
            id: EngineInstanceId::new(instance_id)?,
            adapter_id: manifest.adapter_id.clone(),
            origin: ValidatedEngineOrigin::local_loopback(api_root)?,
            config_revision,
            request_auth,
        })
    }

    pub fn id(&self) -> &EngineInstanceId {
        &self.id
    }

    pub fn adapter_id(&self) -> &EngineAdapterId {
        &self.adapter_id
    }

    pub fn origin(&self) -> &ValidatedEngineOrigin {
        &self.origin
    }

    pub fn config_revision(&self) -> u64 {
        self.config_revision
    }

    pub fn verified_target(&self) -> VerifiedEngineTarget {
        let key = EngineTargetKey {
            instance_id: self.id.clone(),
            adapter_id: self.adapter_id.clone(),
            origin_fingerprint: self.origin.fingerprint(),
            config_revision: self.config_revision,
        };
        VerifiedEngineTarget {
            key,
            origin: self.origin.clone(),
            request_auth: self.request_auth.clone(),
        }
    }
}

impl fmt::Debug for EngineInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineInstance")
            .field("id", &self.id)
            .field("adapter_id", &self.adapter_id)
            .field("config_revision", &self.config_revision)
            .field("origin", &self.origin)
            .finish()
    }
}

/// Rust-constructed target for one saved engine instance.
///
/// It deliberately has no Serde implementation, so IPC payloads cannot become verified targets.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedEngineTarget {
    key: EngineTargetKey,
    origin: ValidatedEngineOrigin,
    request_auth: EngineRequestAuth,
}

impl VerifiedEngineTarget {
    pub fn external_local(
        instance_id: &str,
        manifest: &InferenceEngineManifest,
        api_root: &str,
        config_revision: u64,
    ) -> Result<Self, EngineTargetError> {
        Ok(
            EngineInstance::external_local(instance_id, manifest, api_root, config_revision)?
                .verified_target(),
        )
    }

    pub fn external_local_with_auth(
        instance_id: &str,
        manifest: &InferenceEngineManifest,
        api_root: &str,
        config_revision: u64,
        request_auth: EngineRequestAuth,
    ) -> Result<Self, EngineTargetError> {
        Ok(EngineInstance::external_local_with_auth(
            instance_id,
            manifest,
            api_root,
            config_revision,
            request_auth,
        )?
        .verified_target())
    }

    pub fn key(&self) -> &EngineTargetKey {
        &self.key
    }

    pub fn adapter_id(&self) -> &EngineAdapterId {
        self.key.adapter_id()
    }

    pub fn origin(&self) -> &ValidatedEngineOrigin {
        &self.origin
    }

    pub(crate) fn authenticate(&self, request: RequestBuilder) -> RequestBuilder {
        self.request_auth.authenticate(request)
    }
}

impl fmt::Debug for VerifiedEngineTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedEngineTarget")
            .field("instance_id", &self.key.instance_id)
            .field("adapter_id", &self.key.adapter_id)
            .field("config_revision", &self.key.config_revision)
            .field("origin", &self.origin)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use hal100_protocol::{
        InferenceAccelerator, InferenceArchitecture, InferenceEngineDescriptor,
        InferenceEngineKind, InferenceEngineSupportStatus, InferenceEngineSupportUnit,
        InferenceModelFormat, InferencePlatform, InferenceProtocol,
    };

    use super::*;

    fn manifest() -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: "official-loopback-api".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Ollama,
                display_name: "Ollama".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Metal],
                model_formats: vec![InferenceModelFormat::Gguf],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::VerifiedExternal,
                evidence: Some(crate::support_evidence_for(
                    InferenceEngineKind::Ollama,
                    Some(InferenceEngineSupportStatus::VerifiedExternal),
                )),
            }],
        }
    }

    #[test]
    fn target_key_binds_instance_adapter_origin_and_revision() {
        let manifest = manifest();
        let first = VerifiedEngineTarget::external_local(
            "backend-ollama-a",
            &manifest,
            "http://127.0.0.1:11434/v1/",
            7,
        )
        .expect("first target");
        let second = VerifiedEngineTarget::external_local(
            "backend-ollama-b",
            &manifest,
            "http://127.0.0.1:21434/v1/",
            7,
        )
        .expect("second target");

        assert_ne!(first.key(), second.key());
        assert_eq!(first.adapter_id(), &manifest.adapter_id);
        assert_eq!(first.origin().fingerprint_hex().len(), 64);
        assert_ne!(
            first.origin().fingerprint_hex(),
            second.origin().fingerprint_hex()
        );
        assert_eq!(
            first
                .origin()
                .endpoint("/api/version")
                .expect("endpoint")
                .as_str(),
            "http://127.0.0.1:11434/api/version"
        );
    }

    #[test]
    fn target_rejects_remote_credentials_queries_fragments_and_unbounded_ids() {
        let manifest = manifest();
        for origin in [
            "https://example.com/v1/",
            "http://localhost:11434/v1/",
            "http://user:secret@127.0.0.1:11434/v1/",
            "http://127.0.0.1:11434/v1/?token=secret",
            "http://127.0.0.1:11434/v1/#fragment",
        ] {
            assert!(
                VerifiedEngineTarget::external_local("backend", &manifest, origin, 1).is_err(),
                "origin should be rejected: {origin}"
            );
        }
        assert!(
            VerifiedEngineTarget::external_local(
                &"x".repeat(MAX_INSTANCE_ID_BYTES + 1),
                &manifest,
                "http://127.0.0.1:11434/v1/",
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn request_auth_is_validated_and_never_rendered() {
        let auth = EngineRequestAuth::bearer("top-secret-token").expect("bearer auth");
        let target = VerifiedEngineTarget::external_local_with_auth(
            "backend",
            &manifest(),
            "http://127.0.0.1:11434/v1/",
            1,
            auth,
        )
        .expect("target");

        let rendered = format!("{target:?}");
        assert!(!rendered.contains("top-secret-token"));
        assert!(EngineRequestAuth::bearer("bad\nvalue").is_err());
    }
}
