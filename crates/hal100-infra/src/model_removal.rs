use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use hal100_protocol::{
    LocalModelSummary, ModelOwnership, ModelRemovalKind, ModelRemovalPlan, ModelRemovalResult,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{AGENT_MODEL_ID, Database, DatabaseError};

const REMOVAL_PLAN_TTL_MS: i64 = 5 * 60 * 1_000;
const MAX_MODEL_ID_BYTES: usize = 128;

#[derive(Debug, Error)]
pub enum ModelRemovalError {
    #[error("模型标识无效")]
    InvalidModelId,
    #[error("模型不存在或已从索引移除")]
    ModelNotFound,
    #[error("当前模型正在运行，请先停止或切换模型")]
    ActiveModel,
    #[error("HAL100 Agent 内置模型是当前 Agent 运行依赖，首版不允许从模型库移除")]
    ProtectedAgentModel,
    #[error("托管模型路径不属于 HAL100，已拒绝文件操作")]
    UnsafeManagedPath,
    #[error("模型移除计划不存在、已使用或已被替换")]
    PlanNotFound,
    #[error("模型移除计划已过期，请重新预览")]
    PlanExpired,
    #[error("确认后模型记录或文件发生变化，请重新预览")]
    ModelChangedAfterPreview,
    #[error("无法将托管模型移到系统废纸篓")]
    TrashFailed,
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("模型文件检查失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("模型移除计划锁已损坏")]
    LockPoisoned,
}

pub struct ModelRemovalManager {
    database: Arc<Database>,
    storage_root: PathBuf,
    pending: Mutex<Option<PendingRemoval>>,
}

struct PendingRemoval {
    plan: ModelRemovalPlan,
    expected_path: PathBuf,
    expected_file_size: Option<u64>,
}

impl ModelRemovalManager {
    pub fn new(database: Arc<Database>, storage_root: PathBuf) -> Self {
        Self {
            database,
            storage_root,
            pending: Mutex::new(None),
        }
    }

    pub fn plan_removal(
        &self,
        model_id: &str,
        active_model_id: Option<&str>,
    ) -> Result<ModelRemovalPlan, ModelRemovalError> {
        validate_model_id(model_id)?;
        if model_id == AGENT_MODEL_ID {
            return Err(ModelRemovalError::ProtectedAgentModel);
        }
        if active_model_id == Some(model_id) {
            return Err(ModelRemovalError::ActiveModel);
        }
        let model = self
            .database
            .local_model(model_id)?
            .ok_or(ModelRemovalError::ModelNotFound)?;
        let path = PathBuf::from(&model.path);
        let (removal_kind, expected_file_size, source_file_preserved, action_summary) =
            match model.ownership {
                ModelOwnership::Managed => {
                    self.validate_managed_path(&path, false)?;
                    match fs::symlink_metadata(&path) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            return Err(ModelRemovalError::UnsafeManagedPath);
                        }
                        Ok(metadata) if metadata.is_file() => (
                            ModelRemovalKind::MoveManagedFileToTrash,
                            Some(metadata.len()),
                            false,
                            "将 HAL100 托管的模型文件移到系统废纸篓，并从模型库移除索引".to_owned(),
                        ),
                        Ok(_) => return Err(ModelRemovalError::UnsafeManagedPath),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
                            ModelRemovalKind::RemoveMissingManagedIndex,
                            None,
                            false,
                            "模型文件已经不存在；只清理 HAL100 中的失效托管索引".to_owned(),
                        ),
                        Err(error) => return Err(error.into()),
                    }
                }
                ModelOwnership::External => (
                    ModelRemovalKind::RemoveExternalIndex,
                    None,
                    true,
                    "只从 HAL100 模型库移除外部索引；不会移动、修改或删除源文件".to_owned(),
                ),
            };
        let plan = ModelRemovalPlan {
            plan_id: Uuid::new_v4().to_string(),
            expires_at_ms: now_ms().saturating_add(REMOVAL_PLAN_TTL_MS),
            model_id: model.id,
            display_name: model.display_name,
            ownership: model.ownership,
            size_bytes: model.size_bytes,
            removal_kind,
            action_summary,
            source_file_preserved,
            requires_confirmation: true,
        };
        *self
            .pending
            .lock()
            .map_err(|_| ModelRemovalError::LockPoisoned)? = Some(PendingRemoval {
            plan: plan.clone(),
            expected_path: path,
            expected_file_size,
        });
        Ok(plan)
    }

    pub fn discard_plan(&self, plan_id: &str) -> Result<bool, ModelRemovalError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| ModelRemovalError::LockPoisoned)?;
        if pending
            .as_ref()
            .is_some_and(|pending| pending.plan.plan_id == plan_id)
        {
            pending.take();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn plan(&self, plan_id: &str) -> Result<ModelRemovalPlan, ModelRemovalError> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| ModelRemovalError::LockPoisoned)?;
        let plan = pending
            .as_ref()
            .filter(|pending| pending.plan.plan_id == plan_id && pending.plan.requires_confirmation)
            .map(|pending| pending.plan.clone())
            .ok_or(ModelRemovalError::PlanNotFound)?;
        if now_ms() > plan.expires_at_ms {
            return Err(ModelRemovalError::PlanExpired);
        }
        Ok(plan)
    }

    pub fn apply_removal(
        &self,
        plan_id: &str,
        active_model_id: Option<&str>,
    ) -> Result<ModelRemovalResult, ModelRemovalError> {
        self.apply_with_trash(plan_id, active_model_id, |path| {
            trash::delete(path).map_err(|_| ModelRemovalError::TrashFailed)
        })
    }

    fn apply_with_trash(
        &self,
        plan_id: &str,
        active_model_id: Option<&str>,
        trash_file: impl FnOnce(&Path) -> Result<(), ModelRemovalError>,
    ) -> Result<ModelRemovalResult, ModelRemovalError> {
        let pending = {
            let mut slot = self
                .pending
                .lock()
                .map_err(|_| ModelRemovalError::LockPoisoned)?;
            if !slot
                .as_ref()
                .is_some_and(|pending| pending.plan.plan_id == plan_id)
            {
                return Err(ModelRemovalError::PlanNotFound);
            }
            slot.take().expect("matching model removal plan")
        };
        if now_ms() > pending.plan.expires_at_ms {
            return Err(ModelRemovalError::PlanExpired);
        }
        if active_model_id == Some(pending.plan.model_id.as_str()) {
            return Err(ModelRemovalError::ActiveModel);
        }
        let model = self
            .database
            .local_model(&pending.plan.model_id)?
            .ok_or(ModelRemovalError::ModelNotFound)?;
        self.validate_unchanged_model(&pending, &model)?;

        if pending.plan.removal_kind == ModelRemovalKind::MoveManagedFileToTrash {
            self.validate_managed_path(&pending.expected_path, true)?;
            let metadata = fs::symlink_metadata(&pending.expected_path)
                .map_err(|_| ModelRemovalError::ModelChangedAfterPreview)?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || Some(metadata.len()) != pending.expected_file_size
            {
                return Err(ModelRemovalError::ModelChangedAfterPreview);
            }
            trash_file(&pending.expected_path)?;
        } else if pending.plan.removal_kind == ModelRemovalKind::RemoveMissingManagedIndex
            && pending.expected_path.exists()
        {
            return Err(ModelRemovalError::ModelChangedAfterPreview);
        }

        let removed = self.database.remove_local_model(
            &pending.plan.model_id,
            pending.plan.ownership,
            &pending.expected_path,
            pending.plan.removal_kind,
            now_ms(),
        )?;
        if !removed {
            return Err(ModelRemovalError::ModelChangedAfterPreview);
        }
        Ok(ModelRemovalResult {
            removed: true,
            model_id: pending.plan.model_id,
            display_name: pending.plan.display_name,
            ownership: pending.plan.ownership,
            removal_kind: pending.plan.removal_kind,
            source_file_preserved: pending.plan.source_file_preserved,
        })
    }

    fn validate_unchanged_model(
        &self,
        pending: &PendingRemoval,
        model: &LocalModelSummary,
    ) -> Result<(), ModelRemovalError> {
        if model.id != pending.plan.model_id
            || model.display_name != pending.plan.display_name
            || model.ownership != pending.plan.ownership
            || model.size_bytes != pending.plan.size_bytes
            || Path::new(&model.path) != pending.expected_path
        {
            return Err(ModelRemovalError::ModelChangedAfterPreview);
        }
        Ok(())
    }

    fn validate_managed_path(
        &self,
        candidate: &Path,
        require_file: bool,
    ) -> Result<(), ModelRemovalError> {
        let managed_root = self.storage_root.join("managed");
        let relative = candidate
            .strip_prefix(&managed_root)
            .map_err(|_| ModelRemovalError::UnsafeManagedPath)?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(ModelRemovalError::UnsafeManagedPath);
        }

        let mut cursor = managed_root.clone();
        for component in relative.components() {
            cursor.push(component.as_os_str());
            match fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(ModelRemovalError::UnsafeManagedPath);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !require_file => {
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ModelRemovalError::ModelChangedAfterPreview);
                }
                Err(error) => return Err(error.into()),
            }
        }
        if require_file {
            let canonical_root = managed_root
                .canonicalize()
                .map_err(|_| ModelRemovalError::UnsafeManagedPath)?;
            let canonical_candidate = candidate
                .canonicalize()
                .map_err(|_| ModelRemovalError::ModelChangedAfterPreview)?;
            if !canonical_candidate.starts_with(canonical_root) {
                return Err(ModelRemovalError::UnsafeManagedPath);
            }
        }
        Ok(())
    }
}

fn validate_model_id(model_id: &str) -> Result<(), ModelRemovalError> {
    if model_id.is_empty()
        || model_id.len() > MAX_MODEL_ID_BYTES
        || model_id.chars().any(char::is_control)
    {
        return Err(ModelRemovalError::InvalidModelId);
    }
    Ok(())
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
    use super::*;
    use hal100_protocol::{LocalModelState, ModelSource};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hal100-model-removal-test-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&path).expect("create model removal test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn model(id: &str, path: &Path, ownership: ModelOwnership) -> LocalModelSummary {
        LocalModelSummary {
            id: id.to_owned(),
            display_name: format!("Model {id}"),
            format: "gguf".to_owned(),
            quantization: Some("Q4_K_M".to_owned()),
            source: if ownership == ModelOwnership::Managed {
                ModelSource::HuggingFace
            } else {
                ModelSource::LocalFile
            },
            repository: None,
            revision: None,
            file_name: "model.gguf".to_owned(),
            ownership,
            license: None,
            state: LocalModelState::Ready,
            path: path.display().to_string(),
            size_bytes: fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0),
        }
    }

    fn fixture() -> (TestDirectory, Arc<Database>, ModelRemovalManager) {
        let directory = TestDirectory::new();
        let database = Arc::new(Database::open(directory.0.join("hal100.sqlite")).expect("DB"));
        let storage_root = directory.0.join("models");
        fs::create_dir_all(storage_root.join("managed/source/repo/hash"))
            .expect("managed model directory");
        let manager = ModelRemovalManager::new(database.clone(), storage_root);
        (directory, database, manager)
    }

    #[test]
    fn external_removal_only_drops_the_index() {
        let (directory, database, manager) = fixture();
        let source = directory.0.join("external.gguf");
        fs::write(&source, b"GGUF external model").expect("external model");
        let model = model("external-1", &source, ModelOwnership::External);
        database
            .upsert_local_model(&model, now_ms())
            .expect("index external model");

        let plan = manager
            .plan_removal(&model.id, None)
            .expect("external removal plan");
        assert_eq!(plan.removal_kind, ModelRemovalKind::RemoveExternalIndex);
        let result = manager
            .apply_with_trash(&plan.plan_id, None, |_| {
                panic!("external file must not be trashed")
            })
            .expect("remove external index");

        assert!(result.source_file_preserved);
        assert!(source.exists());
        assert!(database.local_model(&model.id).expect("lookup").is_none());
    }

    #[test]
    fn managed_removal_revalidates_and_moves_only_the_owned_file() {
        let (directory, database, manager) = fixture();
        let source = directory
            .0
            .join("models/managed/source/repo/hash/model.gguf");
        fs::write(&source, b"GGUF managed model").expect("managed model");
        let model = model("managed-1", &source, ModelOwnership::Managed);
        database
            .upsert_local_model(&model, now_ms())
            .expect("index managed model");
        let trashed = directory.0.join("fake-trash.gguf");

        let plan = manager
            .plan_removal(&model.id, None)
            .expect("managed removal plan");
        let result = manager
            .apply_with_trash(&plan.plan_id, None, |path| {
                fs::rename(path, &trashed)?;
                Ok(())
            })
            .expect("remove managed model");

        assert_eq!(
            result.removal_kind,
            ModelRemovalKind::MoveManagedFileToTrash
        );
        assert!(!source.exists());
        assert!(trashed.exists());
        assert!(database.local_model(&model.id).expect("lookup").is_none());
    }

    #[test]
    fn refuses_active_models_and_managed_paths_outside_the_owned_root() {
        let (directory, database, manager) = fixture();
        assert!(matches!(
            manager.plan_removal(AGENT_MODEL_ID, None),
            Err(ModelRemovalError::ProtectedAgentModel)
        ));
        let source = directory.0.join("outside.gguf");
        fs::write(&source, b"GGUF outside model").expect("outside model");
        let model = model("managed-outside", &source, ModelOwnership::Managed);
        database
            .upsert_local_model(&model, now_ms())
            .expect("index unsafe model");

        assert!(matches!(
            manager.plan_removal(&model.id, Some(&model.id)),
            Err(ModelRemovalError::ActiveModel)
        ));
        assert!(matches!(
            manager.plan_removal(&model.id, None),
            Err(ModelRemovalError::UnsafeManagedPath)
        ));
        assert!(source.exists());
    }

    #[test]
    fn missing_managed_file_can_only_remove_its_stale_index() {
        let (_directory, database, manager) = fixture();
        let missing = manager
            .storage_root
            .join("managed/source/repo/hash/missing.gguf");
        let model = LocalModelSummary {
            size_bytes: 123,
            path: missing.display().to_string(),
            ..model("managed-missing", &missing, ModelOwnership::Managed)
        };
        database
            .upsert_local_model(&model, now_ms())
            .expect("index missing model");

        let plan = manager
            .plan_removal(&model.id, None)
            .expect("missing model cleanup plan");
        assert_eq!(
            plan.removal_kind,
            ModelRemovalKind::RemoveMissingManagedIndex
        );
        manager
            .apply_with_trash(&plan.plan_id, None, |_| {
                panic!("missing file has nothing to trash")
            })
            .expect("remove missing index");
        assert!(database.local_model(&model.id).expect("lookup").is_none());
    }
}
