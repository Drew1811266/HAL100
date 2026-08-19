use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use hal100_protocol::{
    GgufImportPlan, GgufImportResult, LocalModelState, LocalModelSummary, ModelOwnership,
    ModelSource,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{Database, DatabaseError};

const GGUF_HEADER_BYTES: usize = 24;
const IMPORT_PLAN_TTL_MS: i64 = 5 * 60 * 1000;
const MAX_TENSOR_COUNT: u64 = 10_000_000;
const MAX_METADATA_COUNT: u64 = 1_000_000;

#[derive(Debug, Error)]
pub enum GgufImportError {
    #[error("只能导入扩展名为 .gguf 的文件")]
    InvalidExtension,
    #[error("不能导入符号链接，请选择实际 GGUF 文件")]
    Symlink,
    #[error("所选路径不是常规文件")]
    NotRegularFile,
    #[error("GGUF 文件头不完整")]
    HeaderTooShort,
    #[error("文件不包含有效的 GGUF 标识")]
    InvalidMagic,
    #[error("暂不支持 GGUF v{0}，当前仅支持 v2 和 v3")]
    UnsupportedVersion(u32),
    #[error("GGUF 文件头中的计数超出安全范围")]
    InvalidHeaderCounts,
    #[error("该文件已经存在于 HAL100 模型库中")]
    AlreadyIndexed,
    #[error("导入确认计划不存在或已被使用")]
    PlanNotFound,
    #[error("导入确认计划已过期，请重新选择文件")]
    PlanExpired,
    #[error("确认前源文件发生了变化，请重新检查")]
    SourceChanged,
    #[error("文件路径无法安全显示")]
    NonUtf8Path,
    #[error("文件大小超出本地数据库支持范围")]
    FileTooLarge,
    #[error("GGUF 文件访问失败：{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("GGUF 导入状态锁已损坏")]
    LockPoisoned,
}

pub struct GgufImportManager {
    database: Arc<Database>,
    pending: Mutex<Option<PendingImport>>,
}

#[derive(Debug, Clone)]
struct PendingImport {
    plan: GgufImportPlan,
    canonical_path: PathBuf,
    modified_at_ms: i64,
    sha256: [u8; 32],
}

#[derive(Debug)]
struct InspectedGguf {
    canonical_path: PathBuf,
    source_path: String,
    display_name: String,
    file_name: String,
    size_bytes: u64,
    modified_at_ms: i64,
    version: u32,
    tensor_count: u64,
    metadata_count: u64,
    quantization: Option<String>,
    sha256: [u8; 32],
}

impl GgufImportManager {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            database,
            pending: Mutex::new(None),
        }
    }

    pub fn plan_import(&self, selected_path: &Path) -> Result<GgufImportPlan, GgufImportError> {
        let inspected = inspect_gguf(selected_path)?;
        if self
            .database
            .model_path_is_indexed(&inspected.canonical_path)?
        {
            return Err(GgufImportError::AlreadyIndexed);
        }
        let now = now_ms();
        let plan = GgufImportPlan {
            plan_id: Uuid::new_v4().to_string(),
            expires_at_ms: now.saturating_add(IMPORT_PLAN_TTL_MS),
            source_path: inspected.source_path,
            display_name: inspected.display_name,
            file_name: inspected.file_name,
            size_bytes: inspected.size_bytes,
            gguf_version: inspected.version,
            tensor_count: inspected.tensor_count,
            metadata_count: inspected.metadata_count,
            quantization: inspected.quantization,
            ownership: ModelOwnership::External,
            action_summary: "只在 HAL100 中建立外部模型索引；不复制、不移动、不删除源文件"
                .to_owned(),
            requires_confirmation: true,
        };
        let pending = PendingImport {
            plan: plan.clone(),
            canonical_path: inspected.canonical_path,
            modified_at_ms: inspected.modified_at_ms,
            sha256: inspected.sha256,
        };
        *self
            .pending
            .lock()
            .map_err(|_| GgufImportError::LockPoisoned)? = Some(pending);
        Ok(plan)
    }

    pub fn apply_import(&self, plan_id: &str) -> Result<GgufImportResult, GgufImportError> {
        let pending = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| GgufImportError::LockPoisoned)?;
            if !pending
                .as_ref()
                .is_some_and(|pending| pending.plan.plan_id == plan_id)
            {
                return Err(GgufImportError::PlanNotFound);
            }
            pending.take().expect("matching pending import exists")
        };
        if now_ms() > pending.plan.expires_at_ms {
            return Err(GgufImportError::PlanExpired);
        }

        let inspected =
            inspect_gguf(&pending.canonical_path).map_err(|_| GgufImportError::SourceChanged)?;
        if inspected.canonical_path != pending.canonical_path
            || inspected.size_bytes != pending.plan.size_bytes
            || inspected.modified_at_ms != pending.modified_at_ms
            || inspected.version != pending.plan.gguf_version
            || inspected.tensor_count != pending.plan.tensor_count
            || inspected.metadata_count != pending.plan.metadata_count
            || inspected.sha256 != pending.sha256
        {
            return Err(GgufImportError::SourceChanged);
        }
        if self
            .database
            .model_path_is_indexed(&pending.canonical_path)?
        {
            return Err(GgufImportError::AlreadyIndexed);
        }

        let model = LocalModelSummary {
            id: format!("external-{}", Uuid::new_v4().simple()),
            display_name: pending.plan.display_name,
            format: "gguf".to_owned(),
            quantization: pending.plan.quantization,
            source: ModelSource::LocalFile,
            repository: None,
            revision: None,
            file_name: pending.plan.file_name,
            ownership: ModelOwnership::External,
            license: None,
            state: LocalModelState::Ready,
            path: pending.plan.source_path,
            size_bytes: pending.plan.size_bytes,
        };
        self.database.upsert_external_model(
            &model,
            pending.modified_at_ms,
            &pending.sha256,
            now_ms(),
        )?;
        Ok(GgufImportResult {
            imported: true,
            model,
        })
    }

    pub fn library(
        &self,
        model_storage_path: &Path,
    ) -> Result<hal100_protocol::ModelLibrary, GgufImportError> {
        self.database.refresh_local_model_states()?;
        self.database
            .model_library(model_storage_path)
            .map_err(Into::into)
    }
}

fn inspect_gguf(path: &Path) -> Result<InspectedGguf, GgufImportError> {
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("gguf"))
    {
        return Err(GgufImportError::InvalidExtension);
    }
    let link_metadata = fs::symlink_metadata(path)?;
    if link_metadata.file_type().is_symlink() {
        return Err(GgufImportError::Symlink);
    }
    if !link_metadata.file_type().is_file() {
        return Err(GgufImportError::NotRegularFile);
    }
    let canonical_path = fs::canonicalize(path)?;
    let source_path = canonical_path
        .to_str()
        .ok_or(GgufImportError::NonUtf8Path)?
        .to_owned();
    let file_name = canonical_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(GgufImportError::NonUtf8Path)?
        .to_owned();

    let mut file = File::open(&canonical_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(GgufImportError::NotRegularFile);
    }
    if metadata.len() < GGUF_HEADER_BYTES as u64 {
        return Err(GgufImportError::HeaderTooShort);
    }
    i64::try_from(metadata.len()).map_err(|_| GgufImportError::FileTooLarge)?;
    let modified_at_ms = system_time_ms(metadata.modified()?);
    let mut header = [0_u8; GGUF_HEADER_BYTES];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"GGUF" {
        return Err(GgufImportError::InvalidMagic);
    }
    let version = u32::from_le_bytes(header[4..8].try_into().expect("fixed GGUF version bytes"));
    if !matches!(version, 2 | 3) {
        return Err(GgufImportError::UnsupportedVersion(version));
    }
    let tensor_count = u64::from_le_bytes(
        header[8..16]
            .try_into()
            .expect("fixed GGUF tensor count bytes"),
    );
    let metadata_count = u64::from_le_bytes(
        header[16..24]
            .try_into()
            .expect("fixed GGUF metadata count bytes"),
    );
    if tensor_count == 0
        || tensor_count > MAX_TENSOR_COUNT
        || metadata_count == 0
        || metadata_count > MAX_METADATA_COUNT
    {
        return Err(GgufImportError::InvalidHeaderCounts);
    }

    let mut hasher = Sha256::new();
    hasher.update(header);
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let final_metadata = file.metadata()?;
    if final_metadata.len() != metadata.len()
        || system_time_ms(final_metadata.modified()?) != modified_at_ms
    {
        return Err(GgufImportError::SourceChanged);
    }
    let sha256 = hasher.finalize().into();

    let quantization = quantization_from_file_name(&file_name);
    let display_name = display_name_from_file_name(&file_name, quantization.as_deref());
    Ok(InspectedGguf {
        canonical_path,
        source_path,
        display_name,
        file_name,
        size_bytes: metadata.len(),
        modified_at_ms,
        version,
        tensor_count,
        metadata_count,
        quantization,
        sha256,
    })
}

pub(crate) fn validate_downloaded_gguf(path: &Path) -> Result<(), GgufImportError> {
    inspect_gguf(path).map(|_| ())
}

fn system_time_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    system_time_ms(SystemTime::now())
}

pub(crate) fn quantization_from_file_name(file_name: &str) -> Option<String> {
    const QUANTIZATIONS: &[&str] = &[
        "IQ4_XS", "IQ3_XXS", "IQ3_XS", "IQ2_XXS", "IQ2_XS", "Q3_K_S", "Q3_K_M", "Q3_K_L", "Q4_K_S",
        "Q4_K_M", "Q5_K_S", "Q5_K_M", "Q2_K", "Q4_0", "Q4_1", "Q5_0", "Q5_1", "Q6_K", "Q8_0",
        "F16", "BF16", "F32",
    ];
    let upper = file_name.to_ascii_uppercase();
    QUANTIZATIONS
        .iter()
        .find(|quantization| upper.contains(**quantization))
        .map(|value| (*value).to_owned())
}

pub(crate) fn display_name_from_file_name(file_name: &str, quantization: Option<&str>) -> String {
    let stem = file_name
        .strip_suffix(".gguf")
        .or_else(|| file_name.strip_suffix(".GGUF"))
        .unwrap_or(file_name);
    let without_quantization = quantization
        .and_then(|quantization| {
            stem.to_ascii_uppercase()
                .rfind(quantization)
                .map(|index| stem[..index].trim_end_matches(['-', '_', '.', ' ']))
        })
        .filter(|value| !value.is_empty())
        .unwrap_or(stem);
    without_quantization.replace(['_', '.'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-gguf-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn gguf(&self, name: &str, version: u32) -> PathBuf {
            let path = self.0.join(name);
            let mut file = File::create(&path).expect("create GGUF fixture");
            file.write_all(b"GGUF").expect("magic");
            file.write_all(&version.to_le_bytes()).expect("version");
            file.write_all(&42_u64.to_le_bytes()).expect("tensors");
            file.write_all(&7_u64.to_le_bytes()).expect("metadata");
            file.write_all(b"fixture payload").expect("payload");
            path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manager(root: &Path) -> (GgufImportManager, Arc<Database>) {
        let database = Arc::new(Database::open(root.join("hal100.sqlite")).expect("database"));
        (GgufImportManager::new(database.clone()), database)
    }

    #[test]
    fn plans_and_confirms_an_external_index_without_copying_the_source() {
        let temp = TestDirectory::new();
        let source = temp.gguf("Qwen-Test-Q4_K_M.gguf", 3);
        let (manager, database) = manager(&temp.0);

        let plan = manager.plan_import(&source).expect("plan import");
        assert!(plan.requires_confirmation);
        assert_eq!(plan.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(plan.display_name, "Qwen-Test");
        assert_eq!(plan.ownership, ModelOwnership::External);
        assert!(source.exists());

        let result = manager.apply_import(&plan.plan_id).expect("apply import");
        assert_eq!(
            result.model.path,
            fs::canonicalize(&source).unwrap().to_str().unwrap()
        );
        assert_eq!(
            database.local_models().expect("models"),
            vec![result.model.clone()]
        );
        assert_eq!(database.audit_event_count().expect("audit count"), 1);
        assert!(source.exists(), "external source must remain untouched");
        assert!(
            manager.apply_import(&plan.plan_id).is_err(),
            "plan is one use"
        );
        assert!(matches!(
            manager.plan_import(&source),
            Err(GgufImportError::AlreadyIndexed)
        ));

        fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .expect("open imported model")
            .write_all(b"changed after import")
            .expect("change imported model");
        let library = manager.library(&temp.0).expect("refreshed library");
        assert_eq!(library.models[0].state, LocalModelState::Changed);
    }

    #[test]
    fn rejects_non_gguf_and_unsupported_versions() {
        let temp = TestDirectory::new();
        let text = temp.0.join("not-a-model.txt");
        fs::write(&text, b"GGUF").expect("text fixture");
        assert!(matches!(
            inspect_gguf(&text),
            Err(GgufImportError::InvalidExtension)
        ));

        let unsupported = temp.gguf("future.gguf", 99);
        assert!(matches!(
            inspect_gguf(&unsupported),
            Err(GgufImportError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn detects_file_change_between_preview_and_confirmation() {
        let temp = TestDirectory::new();
        let source = temp.gguf("model-Q8_0.gguf", 3);
        let (manager, _) = manager(&temp.0);
        let plan = manager.plan_import(&source).expect("plan import");
        fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .expect("open fixture")
            .write_all(b"changed")
            .expect("change fixture");

        assert!(matches!(
            manager.apply_import(&plan.plan_id),
            Err(GgufImportError::SourceChanged)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let temp = TestDirectory::new();
        let source = temp.gguf("source.gguf", 3);
        let link = temp.0.join("link.gguf");
        symlink(source, &link).expect("symlink");
        assert!(matches!(inspect_gguf(&link), Err(GgufImportError::Symlink)));
    }
}
