use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hal100_protocol::ExternalEngineSnapshot;
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    EngineTargetKey, ExternalEngineAdapterError, ExternalInferenceEngineRegistry,
    VerifiedEngineTarget,
};

pub const DEFAULT_ENGINE_DISPLAY_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineObservationPurpose {
    DisplayCache,
    AuthorizationFresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineObservation {
    pub target: EngineTargetKey,
    pub snapshot: ExternalEngineSnapshot,
    pub observed_at_ms: i64,
    pub purpose: EngineObservationPurpose,
}

#[derive(Clone)]
struct CachedObservation {
    snapshot: ExternalEngineSnapshot,
    observed_at_ms: i64,
    inserted_at: Instant,
}

/// Instance-scoped observation service.
///
/// Display reads may use a short cache. Save, reverify, plan, activation, and post-activation
/// checks must call `observe_for_authorization`, which always performs a new inspection.
#[derive(Clone)]
pub struct EngineObservationService {
    registry: Arc<ExternalInferenceEngineRegistry>,
    display_ttl: Duration,
    display_cache: Arc<AsyncMutex<HashMap<EngineTargetKey, CachedObservation>>>,
    instance_flights: Arc<AsyncMutex<HashMap<EngineTargetKey, Arc<AsyncMutex<()>>>>>,
}

impl EngineObservationService {
    pub fn new(registry: Arc<ExternalInferenceEngineRegistry>) -> Self {
        Self::with_display_ttl(registry, DEFAULT_ENGINE_DISPLAY_CACHE_TTL)
    }

    pub fn with_display_ttl(
        registry: Arc<ExternalInferenceEngineRegistry>,
        display_ttl: Duration,
    ) -> Self {
        Self {
            registry,
            display_ttl,
            display_cache: Arc::new(AsyncMutex::new(HashMap::new())),
            instance_flights: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    pub async fn observe_for_display(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<EngineObservation, ExternalEngineAdapterError> {
        let key = target.key().clone();
        let flight = self.instance_flight(&key).await;
        let _instance_guard = flight.lock().await;
        if let Some(cached) = self.cached_display_observation(&key).await {
            return Ok(EngineObservation {
                target: key,
                snapshot: cached.snapshot,
                observed_at_ms: cached.observed_at_ms,
                purpose: EngineObservationPurpose::DisplayCache,
            });
        }
        let snapshot = self.registry.inspect_target(target).await?;
        let observed_at_ms = now_ms();
        self.display_cache.lock().await.insert(
            key.clone(),
            CachedObservation {
                snapshot: snapshot.clone(),
                observed_at_ms,
                inserted_at: Instant::now(),
            },
        );
        Ok(EngineObservation {
            target: key,
            snapshot,
            observed_at_ms,
            purpose: EngineObservationPurpose::DisplayCache,
        })
    }

    pub async fn observe_for_authorization(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<EngineObservation, ExternalEngineAdapterError> {
        let key = target.key().clone();
        let flight = self.instance_flight(&key).await;
        let _instance_guard = flight.lock().await;
        let snapshot = self.registry.inspect_target(target).await?;
        let observed_at_ms = now_ms();
        self.display_cache.lock().await.insert(
            key.clone(),
            CachedObservation {
                snapshot: snapshot.clone(),
                observed_at_ms,
                inserted_at: Instant::now(),
            },
        );
        Ok(EngineObservation {
            target: key,
            snapshot,
            observed_at_ms,
            purpose: EngineObservationPurpose::AuthorizationFresh,
        })
    }

    pub async fn invalidate(&self, target: &VerifiedEngineTarget) {
        self.display_cache.lock().await.remove(target.key());
    }

    async fn instance_flight(&self, key: &EngineTargetKey) -> Arc<AsyncMutex<()>> {
        self.instance_flights
            .lock()
            .await
            .entry(key.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    async fn cached_display_observation(&self, key: &EngineTargetKey) -> Option<CachedObservation> {
        let mut cache = self.display_cache.lock().await;
        let cached = cache.get(key)?.clone();
        if cached.inserted_at.elapsed() <= self.display_ttl {
            Some(cached)
        } else {
            cache.remove(key);
            None
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use hal100_protocol::{
        EngineAdapterId, ExternalEngineModelSummary, InferenceAccelerator, InferenceArchitecture,
        InferenceDeployment, InferenceEngineDescriptor, InferenceEngineKind,
        InferenceEngineManifest, InferenceEngineOwnership, InferenceEngineSupportStatus,
        InferenceEngineSupportUnit, InferenceModelFormat, InferencePlatform, InferenceProtocol,
    };

    use crate::{EngineInspector, ExternalEngineInspectionFuture, ExternalInferenceEngineAdapter};

    use super::*;

    struct MultiInstanceInspector {
        inspections: AtomicUsize,
        keys: Mutex<HashMap<EngineTargetKey, usize>>,
    }

    impl MultiInstanceInspector {
        fn manifest_value() -> InferenceEngineManifest {
            InferenceEngineManifest {
                adapter_id: EngineAdapterId {
                    engine: InferenceEngineKind::Ollama,
                    variant: "multi-instance-test".to_owned(),
                    contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
                },
                descriptor: InferenceEngineDescriptor {
                    kind: InferenceEngineKind::Ollama,
                    display_name: "multi-instance fixture".to_owned(),
                    ownership: InferenceEngineOwnership::External,
                    deployment: InferenceDeployment::Local,
                    protocols: vec![InferenceProtocol::OpenAi],
                    platforms: vec![InferencePlatform::MacOs],
                    architectures: vec![InferenceArchitecture::Aarch64],
                    accelerators: vec![InferenceAccelerator::Cpu],
                    model_formats: vec![InferenceModelFormat::Gguf],
                    managed_lifecycle: false,
                },
                support_units: vec![InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::Ollama,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
                    )),
                }],
            }
        }

        fn inspections_for(&self, key: &EngineTargetKey) -> usize {
            *self
                .keys
                .lock()
                .expect("inspection keys")
                .get(key)
                .unwrap_or(&0)
        }
    }

    impl EngineInspector for MultiInstanceInspector {
        fn manifest(&self) -> InferenceEngineManifest {
            Self::manifest_value()
        }

        fn inspect<'a>(
            &'a self,
            target: &'a VerifiedEngineTarget,
        ) -> ExternalEngineInspectionFuture<'a> {
            self.inspections.fetch_add(1, Ordering::AcqRel);
            *self
                .keys
                .lock()
                .expect("inspection keys")
                .entry(target.key().clone())
                .or_default() += 1;
            let api_root = target.origin().api_root().as_str().to_owned();
            let model_name = format!(
                "model-on-{}",
                target.origin().api_root().port().expect("target port")
            );
            Box::pin(async move {
                Ok(ExternalEngineSnapshot {
                    engine: InferenceEngineKind::Ollama,
                    display_name: "fixture".to_owned(),
                    api_root,
                    version: "test".to_owned(),
                    engine_version_exact: true,
                    models: vec![ExternalEngineModelSummary {
                        name: model_name,
                        digest: "a".repeat(64),
                        size_bytes: 1,
                        format: "gguf".to_owned(),
                        family: None,
                        parameter_size: None,
                        quantization: None,
                        evidence: hal100_protocol::RuntimeProfileEvidence {
                            kind: hal100_protocol::RuntimeProfileEvidenceKind::ContentDigest,
                            algorithm: "ollama-digest".to_owned(),
                            value: "a".repeat(64),
                        },
                    }],
                    model_catalog_complete: true,
                })
            })
        }
    }

    impl ExternalInferenceEngineAdapter for MultiInstanceInspector {}

    fn target(id: &str, port: u16) -> VerifiedEngineTarget {
        VerifiedEngineTarget::external_local(
            id,
            &MultiInstanceInspector::manifest_value(),
            &format!("http://127.0.0.1:{port}/v1/"),
            1,
        )
        .expect("target")
    }

    #[tokio::test]
    async fn display_cache_is_instance_scoped_and_authorization_always_reads_fresh() {
        let inspector = Arc::new(MultiInstanceInspector {
            inspections: AtomicUsize::new(0),
            keys: Mutex::new(HashMap::new()),
        });
        let registry = Arc::new(
            ExternalInferenceEngineRegistry::new(vec![inspector.clone()])
                .expect("external registry"),
        );
        let service = EngineObservationService::with_display_ttl(registry, Duration::from_secs(60));
        let first = target("backend-a", 11434);
        let second = target("backend-b", 21434);

        let (left, right) = tokio::join!(
            service.observe_for_display(&first),
            service.observe_for_display(&first),
        );
        assert_eq!(left.expect("left").snapshot, right.expect("right").snapshot);
        assert_eq!(inspector.inspections_for(first.key()), 1);

        let second_observation = service
            .observe_for_display(&second)
            .await
            .expect("second observation");
        assert_eq!(inspector.inspections_for(second.key()), 1);
        assert_ne!(
            second_observation.snapshot.models[0].name,
            service
                .observe_for_display(&first)
                .await
                .expect("cached first")
                .snapshot
                .models[0]
                .name
        );

        let fresh = service
            .observe_for_authorization(&first)
            .await
            .expect("fresh observation");
        assert_eq!(fresh.purpose, EngineObservationPurpose::AuthorizationFresh);
        assert_eq!(inspector.inspections_for(first.key()), 2);
        assert_eq!(inspector.inspections.load(Ordering::Acquire), 3);
    }
}
