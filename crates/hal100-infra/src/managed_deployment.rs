use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use hal100_core::ExternalAgentIntegrationId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    BoundedCommandError, BoundedCommandRunner, Database, DatabaseError, PendingPlanError,
    PendingPlanStore,
};

const PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const METADATA_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_PACKAGE_JSON_BYTES: u64 = 64 * 1024;
const MAX_PACKAGE_LOCK_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PACKAGE_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_LOCK_PACKAGE_ENTRIES: usize = 2_048;
const NPM_REGISTRY: &str = "https://registry.npmjs.org/";
const PI_PACKAGE_NAME: &str = "@earendil-works/pi-coding-agent";
const PI_PACKAGE_VERSION: &str = "0.84.2";
const PI_PACKAGE_SPEC: &str = "@earendil-works/pi-coding-agent@0.84.2";
const PI_PACKAGE_INTEGRITY: &str = "sha512-l4E+B7hgXKWddRo8bC/eSue2aWZjEgJ9xIpf5p0Og+lq8a2TArCwJ0HCoCPCgaBP/tN4zbYH/wOwvx9pJpeLCA==";
const PI_PACKAGE_BIN: &str = "dist/cli.js";
const PI_PACKAGE_ARCHIVE: &str = "earendil-works-pi-coding-agent-0.84.2.tgz";
const PI_DEPENDENCY_CLOSURE_SHA256: &str =
    "4898c398887684b0fd367f15e75d01b98305a97db2fc805e9ebb0560d2520c37";
const RUNTIME_PACKAGE_JSON: &str = r#"{
  "name": "hal100-managed-pi-runtime",
  "version": "1.0.0",
  "private": true,
  "dependencies": {
    "@earendil-works/pi-coding-agent": "file:earendil-works-pi-coding-agent-0.84.2.tgz"
  }
}
"#;
const LOCK_FINGERPRINT_FIELDS: &[&str] = &[
    "version",
    "resolved",
    "integrity",
    "dependencies",
    "optionalDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "engines",
    "os",
    "cpu",
    "bin",
    "optional",
    "dev",
    "hasInstallScript",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedExternalAgentInstallPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub integration_id: ExternalAgentIntegrationId,
    pub display_name: &'static str,
    pub package_name: &'static str,
    pub package_version: &'static str,
    pub install_scope: &'static str,
    pub lifecycle_notes: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedExternalAgentInstallResult {
    pub integration_id: ExternalAgentIntegrationId,
    pub package_version: &'static str,
    pub managed_binary: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedExternalAgentRemovalPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub integration_id: ExternalAgentIntegrationId,
    pub display_name: &'static str,
    pub package_version: &'static str,
    pub removal_scope: &'static str,
    pub lifecycle_notes: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedExternalAgentRemovalResult {
    pub integration_id: ExternalAgentIntegrationId,
    pub removed: bool,
    pub moved_to_trash: bool,
    pub user_installations_preserved: bool,
}

#[derive(Debug, Error)]
pub enum ManagedExternalAgentDeploymentError {
    #[error("当前外部 Agent 尚无经过验收的受管部署配方")]
    RecipeUnavailable,
    #[error("外部 Agent 已安装；HAL100 不会覆盖现有安装")]
    AlreadyInstalled,
    #[error("未找到 HAL100 私有安装；不会操作用户自行安装的外部 Agent")]
    ManagedInstallationNotFound,
    #[error("未找到受支持的 npm；HAL100 不会修改 PATH 或安装系统级依赖")]
    PackageManagerUnavailable,
    #[error("官方包元数据与 HAL100 固定部署配方不一致")]
    PackageMetadataMismatch,
    #[error("依赖闭包与 HAL100 固定部署配方不一致")]
    DependencyClosureMismatch,
    #[error("受管安装产物未通过版本和入口验证")]
    VerificationFailed,
    #[error("受管安装目录不安全")]
    UnsafeInstallRoot,
    #[error("私有安装在确认后发生变化，请重新生成卸载计划")]
    ManagedInstallationChanged,
    #[error("无法将 HAL100 私有安装移入系统废纸篓")]
    TrashFailed,
    #[error("移入废纸篓失败，且无法恢复 HAL100 私有安装目录")]
    RemovalRollbackFailed,
    #[error("受管部署计划不存在、已使用或已经过期")]
    InvalidPlan,
    #[error("受管部署命令执行失败")]
    CommandFailed(#[source] BoundedCommandError),
    #[error("受管部署数据库不可用")]
    Database(#[from] DatabaseError),
    #[error("受管部署文件操作失败")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageMetadata {
    name: String,
    version: String,
    bin: String,
    integrity: String,
}

#[derive(Debug, Clone)]
struct PendingInstall {
    npm_binary: PathBuf,
    npm_canonical: PathBuf,
    metadata: PackageMetadata,
}

#[derive(Debug, Clone)]
struct PendingRemoval {
    runtime_canonical: PathBuf,
    dependency_fingerprint: String,
}

pub struct ManagedExternalAgentDeploymentManager {
    database: Arc<Database>,
    managed_root: PathBuf,
    npm_candidates: Vec<PathBuf>,
    user_pi_candidates: Vec<PathBuf>,
    metadata_runner: BoundedCommandRunner,
    install_runner: BoundedCommandRunner,
    pending: PendingPlanStore<PendingInstall>,
    pending_removal: PendingPlanStore<PendingRemoval>,
    expected_package_integrity: String,
    expected_dependency_fingerprint: String,
}

impl ManagedExternalAgentDeploymentManager {
    pub fn for_macos(
        database: Arc<Database>,
        home_directory: &Path,
        app_data_directory: &Path,
    ) -> Self {
        Self::with_user_pi_candidates(
            database,
            app_data_directory.join("external-agents"),
            vec![
                home_directory.join(".local/bin/npm"),
                home_directory.join(".npm-global/bin/npm"),
                home_directory.join(".volta/bin/npm"),
                home_directory.join(".bun/bin/npm"),
                PathBuf::from("/opt/homebrew/bin/npm"),
                PathBuf::from("/usr/local/bin/npm"),
            ],
            vec![
                home_directory.join(".local/bin/pi"),
                home_directory.join(".bun/bin/pi"),
                home_directory.join(".npm-global/bin/pi"),
                PathBuf::from("/opt/homebrew/bin/pi"),
                PathBuf::from("/usr/local/bin/pi"),
            ],
        )
    }

    pub fn new(
        database: Arc<Database>,
        managed_root: PathBuf,
        npm_candidates: Vec<PathBuf>,
    ) -> Self {
        Self::with_user_pi_candidates(database, managed_root, npm_candidates, Vec::new())
    }

    fn with_user_pi_candidates(
        database: Arc<Database>,
        managed_root: PathBuf,
        npm_candidates: Vec<PathBuf>,
        user_pi_candidates: Vec<PathBuf>,
    ) -> Self {
        Self {
            database,
            managed_root,
            npm_candidates,
            user_pi_candidates,
            metadata_runner: BoundedCommandRunner::new(METADATA_TIMEOUT, MAX_COMMAND_OUTPUT_BYTES),
            install_runner: BoundedCommandRunner::new(INSTALL_TIMEOUT, MAX_COMMAND_OUTPUT_BYTES),
            pending: PendingPlanStore::new(PLAN_TTL),
            pending_removal: PendingPlanStore::new(PLAN_TTL),
            expected_package_integrity: PI_PACKAGE_INTEGRITY.to_owned(),
            expected_dependency_fingerprint: PI_DEPENDENCY_CLOSURE_SHA256.to_owned(),
        }
    }

    pub fn managed_pi_binary(&self) -> PathBuf {
        self.integration_runtime().join("node_modules/.bin/pi")
    }

    pub fn managed_pi_installed(&self) -> Result<bool, ManagedExternalAgentDeploymentError> {
        self.runtime_entry_exists()
    }

    pub fn plan_install(
        &self,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ManagedExternalAgentInstallPlan, ManagedExternalAgentDeploymentError> {
        self.require_pi_recipe(integration_id)?;
        if self.runtime_entry_exists()? || self.user_pi_installed() {
            return Err(ManagedExternalAgentDeploymentError::AlreadyInstalled);
        }
        let (npm_binary, npm_canonical) = self.discover_npm()?;
        self.validate_npm(&npm_binary)?;
        let metadata = self.read_registry_metadata(&npm_binary)?;
        validate_recipe_metadata(&metadata)?;
        let ticket = self
            .pending
            .replace(PendingInstall {
                npm_binary,
                npm_canonical,
                metadata,
            })
            .map_err(map_pending_error)?;
        self.database.insert_audit_event(
            "external_agent_install_planned",
            "external_agent",
            "pi-coding-agent",
            &json!({
                "package": PI_PACKAGE_NAME,
                "version": PI_PACKAGE_VERSION,
                "scope": "hal100_private",
                "scriptsDisabled": true,
            })
            .to_string(),
            now_ms(),
        )?;
        Ok(ManagedExternalAgentInstallPlan {
            plan_id: ticket.plan_id,
            expires_at_ms: ticket.expires_at_ms,
            integration_id,
            display_name: "Pi Coding Agent",
            package_name: PI_PACKAGE_NAME,
            package_version: PI_PACKAGE_VERSION,
            install_scope: "HAL100 私有应用数据目录",
            lifecycle_notes: vec![
                "固定官方包名、版本、Registry、包完整性与完整依赖闭包",
                "禁用依赖安装脚本，不申请管理员权限",
                "不修改用户 PATH、HOME、Pi 配置或现有安装",
                "在独立私有目录解析依赖，闭包通过后才验证 CLI 并原子启用",
            ],
        })
    }

    pub fn apply_install(
        &self,
        plan_id: &str,
    ) -> Result<ManagedExternalAgentInstallResult, ManagedExternalAgentDeploymentError> {
        let pending = self.pending.take(plan_id).map_err(map_pending_error)?;
        let result = self.apply_pending_install(&pending);
        let (event_type, summary) = match &result {
            Ok(_) => (
                "external_agent_install_completed",
                json!({
                    "package": PI_PACKAGE_NAME,
                    "version": PI_PACKAGE_VERSION,
                    "scope": "hal100_private",
                    "verified": true,
                }),
            ),
            Err(error) => (
                "external_agent_install_failed",
                json!({
                    "package": PI_PACKAGE_NAME,
                    "version": PI_PACKAGE_VERSION,
                    "errorCode": deployment_error_code(error),
                }),
            ),
        };
        self.database.insert_audit_event(
            event_type,
            "external_agent",
            "pi-coding-agent",
            &summary.to_string(),
            now_ms(),
        )?;
        result
    }

    pub fn discard_install_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, ManagedExternalAgentDeploymentError> {
        self.pending.discard(plan_id).map_err(map_pending_error)
    }

    pub fn plan_removal(
        &self,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ManagedExternalAgentRemovalPlan, ManagedExternalAgentDeploymentError> {
        self.require_pi_recipe(integration_id)?;
        let (runtime_canonical, dependency_fingerprint) = self.verify_removable_runtime()?;
        let ticket = self
            .pending_removal
            .replace(PendingRemoval {
                runtime_canonical,
                dependency_fingerprint,
            })
            .map_err(map_pending_error)?;
        self.database.insert_audit_event(
            "external_agent_removal_planned",
            "external_agent",
            "pi-coding-agent",
            &json!({
                "package": PI_PACKAGE_NAME,
                "version": PI_PACKAGE_VERSION,
                "scope": "hal100_private",
                "recoverable": true,
                "userInstallationsPreserved": true,
            })
            .to_string(),
            now_ms(),
        )?;
        Ok(ManagedExternalAgentRemovalPlan {
            plan_id: ticket.plan_id,
            expires_at_ms: ticket.expires_at_ms,
            integration_id,
            display_name: "Pi Coding Agent",
            package_version: PI_PACKAGE_VERSION,
            removal_scope: "仅 HAL100 私有 Pi 运行时目录",
            lifecycle_notes: vec![
                "重新验证固定版本、入口与完整依赖闭包后才执行",
                "仅移动 HAL100 私有运行时，不触碰用户自行安装的 Pi",
                "不修改 Pi 配置、凭据、会话、HOME 或 PATH",
                "私有运行时移入系统废纸篓，可由用户恢复",
            ],
        })
    }

    pub fn apply_removal(
        &self,
        plan_id: &str,
    ) -> Result<ManagedExternalAgentRemovalResult, ManagedExternalAgentDeploymentError> {
        self.apply_removal_with_trash(plan_id, |path| {
            trash::delete(path).map_err(|_| ManagedExternalAgentDeploymentError::TrashFailed)
        })
    }

    pub fn discard_removal_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, ManagedExternalAgentDeploymentError> {
        self.pending_removal
            .discard(plan_id)
            .map_err(map_pending_error)
    }

    fn apply_removal_with_trash(
        &self,
        plan_id: &str,
        trash_runtime: impl FnOnce(&Path) -> Result<(), ManagedExternalAgentDeploymentError>,
    ) -> Result<ManagedExternalAgentRemovalResult, ManagedExternalAgentDeploymentError> {
        let pending = self
            .pending_removal
            .take(plan_id)
            .map_err(map_pending_error)?;
        let (runtime_canonical, dependency_fingerprint) = self
            .verify_removable_runtime()
            .map_err(map_removal_revalidation_error)?;
        if runtime_canonical != pending.runtime_canonical
            || dependency_fingerprint != pending.dependency_fingerprint
        {
            return Err(ManagedExternalAgentDeploymentError::ManagedInstallationChanged);
        }
        self.database.insert_audit_event(
            "external_agent_removal_started",
            "external_agent",
            "pi-coding-agent",
            &json!({
                "version": PI_PACKAGE_VERSION,
                "scope": "hal100_private",
            })
            .to_string(),
            now_ms(),
        )?;

        let quarantine = self.integration_root().join(format!(
            "HAL100-Pi-Coding-Agent-{PI_PACKAGE_VERSION}-{}",
            Uuid::new_v4().simple()
        ));
        fs::rename(&runtime_canonical, &quarantine)?;
        let removal_result = trash_runtime(&quarantine);
        let result = match removal_result {
            Ok(()) => Ok(ManagedExternalAgentRemovalResult {
                integration_id: ExternalAgentIntegrationId::PiCodingAgent,
                removed: true,
                moved_to_trash: true,
                user_installations_preserved: true,
            }),
            Err(error) => {
                if quarantine.exists() && fs::rename(&quarantine, &runtime_canonical).is_ok() {
                    Err(error)
                } else {
                    Err(ManagedExternalAgentDeploymentError::RemovalRollbackFailed)
                }
            }
        };
        let (event_type, summary) = match &result {
            Ok(_) => (
                "external_agent_removal_completed",
                json!({
                    "version": PI_PACKAGE_VERSION,
                    "scope": "hal100_private",
                    "movedToTrash": true,
                    "userInstallationsPreserved": true,
                }),
            ),
            Err(error) => (
                "external_agent_removal_failed",
                json!({
                    "version": PI_PACKAGE_VERSION,
                    "errorCode": deployment_error_code(error),
                }),
            ),
        };
        let _ = self.database.insert_audit_event(
            event_type,
            "external_agent",
            "pi-coding-agent",
            &summary.to_string(),
            now_ms(),
        );
        result
    }

    fn apply_pending_install(
        &self,
        pending: &PendingInstall,
    ) -> Result<ManagedExternalAgentInstallResult, ManagedExternalAgentDeploymentError> {
        if self.runtime_entry_exists()? || self.user_pi_installed() {
            return Err(ManagedExternalAgentDeploymentError::AlreadyInstalled);
        }
        if fs::canonicalize(&pending.npm_binary).ok().as_ref() != Some(&pending.npm_canonical) {
            return Err(ManagedExternalAgentDeploymentError::PackageManagerUnavailable);
        }
        self.validate_npm(&pending.npm_binary)?;
        let current_metadata = self.read_registry_metadata(&pending.npm_binary)?;
        validate_recipe_metadata(&current_metadata)?;
        if current_metadata != pending.metadata {
            return Err(ManagedExternalAgentDeploymentError::PackageMetadataMismatch);
        }

        self.prepare_install_root()?;
        let staging = self
            .integration_root()
            .join(format!(".staging-{}", Uuid::new_v4().simple()));
        fs::create_dir(&staging)?;
        set_owner_only_directory(&staging)?;
        let result = self.install_into_staging(pending, &staging);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    fn install_into_staging(
        &self,
        pending: &PendingInstall,
        staging: &Path,
    ) -> Result<ManagedExternalAgentInstallResult, ManagedExternalAgentDeploymentError> {
        let npm_parent = pending
            .npm_binary
            .parent()
            .ok_or(ManagedExternalAgentDeploymentError::PackageManagerUnavailable)?;
        let environment = npm_environment(npm_parent);
        let archive = self.pack_verified_archive(&pending.npm_binary, staging, &environment)?;
        fs::write(staging.join("package.json"), RUNTIME_PACKAGE_JSON)?;
        self.install_runner
            .run_utf8_with_env_in_dir(
                &pending.npm_binary,
                &[
                    "install",
                    "--ignore-scripts",
                    "--no-audit",
                    "--no-fund",
                    "--loglevel=error",
                    "--package-lock=true",
                    "--registry",
                    NPM_REGISTRY,
                ],
                &environment,
                Some(staging),
            )
            .map_err(ManagedExternalAgentDeploymentError::CommandFailed)?;

        self.verify_dependency_lock(staging)?;
        fs::remove_file(&archive)?;

        self.verify_runtime(staging, &pending.npm_binary)?;
        let runtime = self.integration_runtime();
        fs::rename(staging, &runtime)?;
        if let Err(error) = self.verify_runtime(&runtime, &pending.npm_binary) {
            let _ = fs::remove_dir_all(&runtime);
            return Err(error);
        }
        Ok(ManagedExternalAgentInstallResult {
            integration_id: ExternalAgentIntegrationId::PiCodingAgent,
            package_version: PI_PACKAGE_VERSION,
            managed_binary: runtime.join("node_modules/.bin/pi"),
        })
    }

    fn pack_verified_archive(
        &self,
        npm_binary: &Path,
        staging: &Path,
        environment: &[(String, OsString)],
    ) -> Result<PathBuf, ManagedExternalAgentDeploymentError> {
        let output = self
            .install_runner
            .run_utf8_with_env_in_dir(
                npm_binary,
                &[
                    "pack",
                    PI_PACKAGE_SPEC,
                    "--silent",
                    "--ignore-scripts",
                    "--registry",
                    NPM_REGISTRY,
                    "--pack-destination",
                    ".",
                ],
                environment,
                Some(staging),
            )
            .map_err(ManagedExternalAgentDeploymentError::CommandFailed)?;
        if output.trim() != PI_PACKAGE_ARCHIVE {
            return Err(ManagedExternalAgentDeploymentError::PackageMetadataMismatch);
        }
        let archive = staging.join(PI_PACKAGE_ARCHIVE);
        let metadata = fs::symlink_metadata(&archive)
            .map_err(|_| ManagedExternalAgentDeploymentError::VerificationFailed)?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_PACKAGE_ARCHIVE_BYTES
        {
            return Err(ManagedExternalAgentDeploymentError::VerificationFailed);
        }
        if archive_sri(&archive)? != self.expected_package_integrity {
            return Err(ManagedExternalAgentDeploymentError::PackageMetadataMismatch);
        }
        Ok(archive)
    }

    fn verify_dependency_lock(
        &self,
        prefix: &Path,
    ) -> Result<String, ManagedExternalAgentDeploymentError> {
        let fingerprint = dependency_closure_fingerprint(&prefix.join("package-lock.json"))?;
        if fingerprint != self.expected_dependency_fingerprint {
            return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
        }
        Ok(fingerprint)
    }

    fn verify_runtime_files(
        &self,
        prefix: &Path,
    ) -> Result<String, ManagedExternalAgentDeploymentError> {
        let runtime_metadata = fs::symlink_metadata(prefix)
            .map_err(|_| ManagedExternalAgentDeploymentError::ManagedInstallationNotFound)?;
        if runtime_metadata.file_type().is_symlink() || !runtime_metadata.is_dir() {
            return Err(ManagedExternalAgentDeploymentError::UnsafeInstallRoot);
        }
        let fingerprint = self.verify_dependency_lock(prefix)?;
        let package_json = prefix
            .join("node_modules")
            .join(PI_PACKAGE_NAME)
            .join("package.json");
        let metadata = fs::symlink_metadata(&package_json)
            .map_err(|_| ManagedExternalAgentDeploymentError::VerificationFailed)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_PACKAGE_JSON_BYTES {
            return Err(ManagedExternalAgentDeploymentError::VerificationFailed);
        }
        let package: Value = serde_json::from_slice(&fs::read(&package_json)?)
            .map_err(|_| ManagedExternalAgentDeploymentError::VerificationFailed)?;
        if package.get("name").and_then(Value::as_str) != Some(PI_PACKAGE_NAME)
            || package.get("version").and_then(Value::as_str) != Some(PI_PACKAGE_VERSION)
            || package
                .get("bin")
                .and_then(|value| value.get("pi"))
                .and_then(Value::as_str)
                != Some(PI_PACKAGE_BIN)
        {
            return Err(ManagedExternalAgentDeploymentError::VerificationFailed);
        }
        let binary = prefix.join("node_modules/.bin/pi");
        let binary_metadata = fs::symlink_metadata(&binary)
            .map_err(|_| ManagedExternalAgentDeploymentError::VerificationFailed)?;
        if !binary_metadata.file_type().is_symlink() {
            return Err(ManagedExternalAgentDeploymentError::VerificationFailed);
        }
        let canonical_package_root =
            fs::canonicalize(prefix.join("node_modules").join(PI_PACKAGE_NAME))?;
        let canonical_binary = fs::canonicalize(&binary)?;
        if !canonical_binary.starts_with(&canonical_package_root) || !canonical_binary.is_file() {
            return Err(ManagedExternalAgentDeploymentError::VerificationFailed);
        }
        Ok(fingerprint)
    }

    fn verify_runtime(
        &self,
        prefix: &Path,
        npm_binary: &Path,
    ) -> Result<(), ManagedExternalAgentDeploymentError> {
        self.verify_runtime_files(prefix)?;
        let binary = prefix.join("node_modules/.bin/pi");
        let npm_parent = npm_binary
            .parent()
            .ok_or(ManagedExternalAgentDeploymentError::VerificationFailed)?;
        let environment = npm_environment(npm_parent);
        let version = self
            .metadata_runner
            .run_utf8_with_env(&binary, &["--version"], &environment)
            .map_err(ManagedExternalAgentDeploymentError::CommandFailed)?;
        if version.trim() != PI_PACKAGE_VERSION {
            return Err(ManagedExternalAgentDeploymentError::VerificationFailed);
        }
        Ok(())
    }

    fn verify_removable_runtime(
        &self,
    ) -> Result<(PathBuf, String), ManagedExternalAgentDeploymentError> {
        let runtime = self.integration_runtime();
        match fs::symlink_metadata(&runtime) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(ManagedExternalAgentDeploymentError::UnsafeInstallRoot);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ManagedExternalAgentDeploymentError::ManagedInstallationNotFound);
            }
            Err(error) => return Err(error.into()),
        }
        ensure_directory_not_symlink(&self.managed_root)?;
        ensure_directory_not_symlink(&self.integration_root())?;
        let integration_canonical = fs::canonicalize(self.integration_root())?;
        let runtime_canonical = fs::canonicalize(&runtime)?;
        if runtime_canonical.parent() != Some(integration_canonical.as_path()) {
            return Err(ManagedExternalAgentDeploymentError::UnsafeInstallRoot);
        }
        let fingerprint = self.verify_runtime_files(&runtime_canonical)?;
        Ok((runtime_canonical, fingerprint))
    }

    fn read_registry_metadata(
        &self,
        npm_binary: &Path,
    ) -> Result<PackageMetadata, ManagedExternalAgentDeploymentError> {
        let output = self
            .metadata_runner
            .run_utf8_with_env_in_dir(
                npm_binary,
                &[
                    "view",
                    PI_PACKAGE_SPEC,
                    "name",
                    "version",
                    "bin",
                    "dist.integrity",
                    "--json",
                    "--registry",
                    NPM_REGISTRY,
                ],
                &npm_environment(
                    npm_binary
                        .parent()
                        .ok_or(ManagedExternalAgentDeploymentError::PackageManagerUnavailable)?,
                ),
                Some(&std::env::temp_dir()),
            )
            .map_err(ManagedExternalAgentDeploymentError::CommandFailed)?;
        let value: Value = serde_json::from_str(&output)
            .map_err(|_| ManagedExternalAgentDeploymentError::PackageMetadataMismatch)?;
        Ok(PackageMetadata {
            name: required_string(&value, "name")?,
            version: required_string(&value, "version")?,
            bin: value
                .get("bin")
                .and_then(|bin| bin.get("pi"))
                .and_then(Value::as_str)
                .ok_or(ManagedExternalAgentDeploymentError::PackageMetadataMismatch)?
                .to_owned(),
            integrity: value
                .get("dist")
                .and_then(|dist| dist.get("integrity"))
                .and_then(Value::as_str)
                .or_else(|| value.get("dist.integrity").and_then(Value::as_str))
                .ok_or(ManagedExternalAgentDeploymentError::PackageMetadataMismatch)?
                .to_owned(),
        })
    }

    fn discover_npm(&self) -> Result<(PathBuf, PathBuf), ManagedExternalAgentDeploymentError> {
        self.npm_candidates
            .iter()
            .find_map(|candidate| {
                let metadata = fs::symlink_metadata(candidate).ok()?;
                (metadata.file_type().is_file() || metadata.file_type().is_symlink())
                    .then(|| fs::canonicalize(candidate).ok())
                    .flatten()
                    .filter(|resolved| resolved.is_file())
                    .map(|resolved| (candidate.clone(), resolved))
            })
            .ok_or(ManagedExternalAgentDeploymentError::PackageManagerUnavailable)
    }

    fn validate_npm(&self, npm_binary: &Path) -> Result<(), ManagedExternalAgentDeploymentError> {
        let version = self
            .metadata_runner
            .run_utf8_with_env_in_dir(
                npm_binary,
                &["--version"],
                &npm_environment(
                    npm_binary
                        .parent()
                        .ok_or(ManagedExternalAgentDeploymentError::PackageManagerUnavailable)?,
                ),
                Some(&std::env::temp_dir()),
            )
            .map_err(ManagedExternalAgentDeploymentError::CommandFailed)?;
        let valid = !version.trim().is_empty()
            && version.trim().split('.').all(|segment| {
                !segment.is_empty() && segment.chars().all(|ch| ch.is_ascii_digit())
            });
        if !valid {
            return Err(ManagedExternalAgentDeploymentError::PackageManagerUnavailable);
        }
        Ok(())
    }

    fn prepare_install_root(&self) -> Result<(), ManagedExternalAgentDeploymentError> {
        ensure_directory_not_symlink(&self.managed_root)?;
        fs::create_dir_all(&self.managed_root)?;
        set_owner_only_directory(&self.managed_root)?;
        let integration_root = self.integration_root();
        ensure_directory_not_symlink(&integration_root)?;
        fs::create_dir_all(&integration_root)?;
        set_owner_only_directory(&integration_root)?;
        Ok(())
    }

    fn require_pi_recipe(
        &self,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<(), ManagedExternalAgentDeploymentError> {
        if integration_id != ExternalAgentIntegrationId::PiCodingAgent {
            return Err(ManagedExternalAgentDeploymentError::RecipeUnavailable);
        }
        Ok(())
    }

    fn integration_root(&self) -> PathBuf {
        self.managed_root.join("pi-coding-agent")
    }

    fn integration_runtime(&self) -> PathBuf {
        self.integration_root().join("runtime")
    }

    fn runtime_entry_exists(&self) -> Result<bool, ManagedExternalAgentDeploymentError> {
        match fs::symlink_metadata(self.integration_runtime()) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn user_pi_installed(&self) -> bool {
        self.user_pi_candidates.iter().any(|candidate| {
            fs::symlink_metadata(candidate)
                .ok()
                .is_some_and(|metadata| {
                    (metadata.file_type().is_file() || metadata.file_type().is_symlink())
                        && fs::canonicalize(candidate)
                            .ok()
                            .is_some_and(|resolved| resolved.is_file())
                })
        })
    }
}

fn validate_recipe_metadata(
    metadata: &PackageMetadata,
) -> Result<(), ManagedExternalAgentDeploymentError> {
    if metadata.name != PI_PACKAGE_NAME
        || metadata.version != PI_PACKAGE_VERSION
        || metadata.bin != PI_PACKAGE_BIN
        || metadata.integrity != PI_PACKAGE_INTEGRITY
    {
        return Err(ManagedExternalAgentDeploymentError::PackageMetadataMismatch);
    }
    Ok(())
}

fn npm_environment(npm_parent: &Path) -> [(String, OsString); 1] {
    [(
        "PATH".to_owned(),
        OsString::from(format!(
            "{}:/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin",
            npm_parent.to_string_lossy()
        )),
    )]
}

fn archive_sri(path: &Path) -> Result<String, ManagedExternalAgentDeploymentError> {
    let mut file = File::open(path)?;
    let mut digest = Sha512::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!(
        "sha512-{}",
        BASE64_STANDARD.encode(digest.finalize())
    ))
}

fn dependency_closure_fingerprint(
    lock_path: &Path,
) -> Result<String, ManagedExternalAgentDeploymentError> {
    let metadata = fs::symlink_metadata(lock_path)
        .map_err(|_| ManagedExternalAgentDeploymentError::DependencyClosureMismatch)?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_PACKAGE_LOCK_BYTES
    {
        return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
    }
    let lock: Value = serde_json::from_slice(&fs::read(lock_path)?)
        .map_err(|_| ManagedExternalAgentDeploymentError::DependencyClosureMismatch)?;
    dependency_closure_fingerprint_value(&lock)
}

fn dependency_closure_fingerprint_value(
    lock: &Value,
) -> Result<String, ManagedExternalAgentDeploymentError> {
    if lock.get("name").and_then(Value::as_str) != Some("hal100-managed-pi-runtime")
        || lock.get("version").and_then(Value::as_str) != Some("1.0.0")
        || lock.get("lockfileVersion").and_then(Value::as_u64) != Some(3)
    {
        return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
    }
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .filter(|packages| packages.len() > 1 && packages.len() <= MAX_LOCK_PACKAGE_ENTRIES)
        .ok_or(ManagedExternalAgentDeploymentError::DependencyClosureMismatch)?;
    let root_dependencies = packages
        .get("")
        .and_then(Value::as_object)
        .and_then(|root| root.get("dependencies"))
        .and_then(Value::as_object)
        .filter(|dependencies| dependencies.len() == 1)
        .ok_or(ManagedExternalAgentDeploymentError::DependencyClosureMismatch)?;
    if root_dependencies
        .get(PI_PACKAGE_NAME)
        .and_then(Value::as_str)
        != Some("file:earendil-works-pi-coding-agent-0.84.2.tgz")
    {
        return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
    }

    let mut closure = BTreeMap::<String, BTreeMap<String, Value>>::new();
    for (package_path, entry) in packages {
        if package_path.is_empty() {
            continue;
        }
        if !safe_lock_package_path(package_path) {
            return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
        }
        let entry = entry
            .as_object()
            .ok_or(ManagedExternalAgentDeploymentError::DependencyClosureMismatch)?;
        let version = required_lock_string(entry, "version")?;
        let resolved = required_lock_string(entry, "resolved")?;
        let integrity = required_lock_string(entry, "integrity")?;
        if !integrity.starts_with("sha512-") || integrity.len() <= "sha512-".len() {
            return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
        }
        let is_top_level = package_path == "node_modules/@earendil-works/pi-coding-agent";
        if is_top_level {
            if version != PI_PACKAGE_VERSION
                || resolved != "file:earendil-works-pi-coding-agent-0.84.2.tgz"
                || integrity != PI_PACKAGE_INTEGRITY
            {
                return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
            }
        } else if !resolved.starts_with(NPM_REGISTRY) {
            return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
        }

        let mut selected = BTreeMap::new();
        for field in LOCK_FINGERPRINT_FIELDS {
            if let Some(value) = entry.get(*field) {
                selected.insert((*field).to_owned(), canonicalize_json(value));
            }
        }
        closure.insert(package_path.clone(), selected);
    }
    if !closure.contains_key("node_modules/@earendil-works/pi-coding-agent") {
        return Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch);
    }
    let canonical = serde_json::to_vec(&closure)
        .map_err(|_| ManagedExternalAgentDeploymentError::DependencyClosureMismatch)?;
    Ok(format!("{:x}", Sha256::digest(&canonical)))
}

fn required_lock_string<'a>(
    entry: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a str, ManagedExternalAgentDeploymentError> {
    entry
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(ManagedExternalAgentDeploymentError::DependencyClosureMismatch)
}

fn safe_lock_package_path(package_path: &str) -> bool {
    let path = Path::new(package_path);
    !path.is_absolute()
        && package_path.starts_with("node_modules/")
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_json(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::to_value(sorted).expect("JSON values always serialize")
        }
        value => value.clone(),
    }
}

fn required_string(
    value: &Value,
    key: &str,
) -> Result<String, ManagedExternalAgentDeploymentError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(ManagedExternalAgentDeploymentError::PackageMetadataMismatch)
}

fn ensure_directory_not_symlink(path: &Path) -> Result<(), ManagedExternalAgentDeploymentError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(ManagedExternalAgentDeploymentError::UnsafeInstallRoot)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

fn map_pending_error(_: PendingPlanError) -> ManagedExternalAgentDeploymentError {
    ManagedExternalAgentDeploymentError::InvalidPlan
}

fn map_removal_revalidation_error(
    error: ManagedExternalAgentDeploymentError,
) -> ManagedExternalAgentDeploymentError {
    match error {
        ManagedExternalAgentDeploymentError::ManagedInstallationNotFound
        | ManagedExternalAgentDeploymentError::DependencyClosureMismatch
        | ManagedExternalAgentDeploymentError::VerificationFailed => {
            ManagedExternalAgentDeploymentError::ManagedInstallationChanged
        }
        error => error,
    }
}

fn deployment_error_code(error: &ManagedExternalAgentDeploymentError) -> &'static str {
    match error {
        ManagedExternalAgentDeploymentError::RecipeUnavailable => "recipe_unavailable",
        ManagedExternalAgentDeploymentError::AlreadyInstalled => "already_installed",
        ManagedExternalAgentDeploymentError::ManagedInstallationNotFound => {
            "managed_installation_not_found"
        }
        ManagedExternalAgentDeploymentError::PackageManagerUnavailable => {
            "package_manager_unavailable"
        }
        ManagedExternalAgentDeploymentError::PackageMetadataMismatch => "package_metadata_mismatch",
        ManagedExternalAgentDeploymentError::DependencyClosureMismatch => {
            "dependency_closure_mismatch"
        }
        ManagedExternalAgentDeploymentError::VerificationFailed => "verification_failed",
        ManagedExternalAgentDeploymentError::UnsafeInstallRoot => "unsafe_install_root",
        ManagedExternalAgentDeploymentError::ManagedInstallationChanged => {
            "managed_installation_changed"
        }
        ManagedExternalAgentDeploymentError::TrashFailed => "trash_failed",
        ManagedExternalAgentDeploymentError::RemovalRollbackFailed => "removal_rollback_failed",
        ManagedExternalAgentDeploymentError::InvalidPlan => "invalid_plan",
        ManagedExternalAgentDeploymentError::CommandFailed(_) => "command_failed",
        ManagedExternalAgentDeploymentError::Database(_) => "database_unavailable",
        ManagedExternalAgentDeploymentError::Io(_) => "io_failed",
    }
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

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hal100-managed-deployment-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fake_lock() -> Value {
        json!({
            "name": "hal100-managed-pi-runtime",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "hal100-managed-pi-runtime",
                    "version": "1.0.0",
                    "dependencies": {
                        (PI_PACKAGE_NAME): "file:earendil-works-pi-coding-agent-0.84.2.tgz"
                    }
                },
                "node_modules/@earendil-works/pi-coding-agent": {
                    "version": PI_PACKAGE_VERSION,
                    "resolved": "file:earendil-works-pi-coding-agent-0.84.2.tgz",
                    "integrity": PI_PACKAGE_INTEGRITY,
                    "bin": { "pi": PI_PACKAGE_BIN }
                }
            }
        })
    }

    fn fixture() -> (TestDirectory, ManagedExternalAgentDeploymentManager) {
        let temp = TestDirectory::new();
        let npm = temp.0.join("bin/npm");
        fs::create_dir_all(npm.parent().expect("npm parent")).expect("bin directory");
        let lock = fake_lock();
        fs::write(
            &npm,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '11.5.2\n'
  exit 0
fi
if [ "$1" = "view" ]; then
  printf '%s\n' '{{"name":"{PI_PACKAGE_NAME}","version":"{PI_PACKAGE_VERSION}","bin":{{"pi":"{PI_PACKAGE_BIN}"}},"dist":{{"integrity":"{PI_PACKAGE_INTEGRITY}"}}}}'
  exit 0
fi
if [ "$1" = "pack" ]; then
  filename='earendil-works-pi-coding-agent-0.84.2.tgz'
  printf 'fixed archive' > "$PWD/$filename"
  printf '%s\n' "$filename"
  exit 0
fi
if [ "$1" = "install" ]; then
  prefix="$PWD"
  package="$prefix/node_modules/@earendil-works/pi-coding-agent"
  mkdir -p "$package/dist" "$prefix/node_modules/.bin"
  printf '%s\n' '{{"name":"{PI_PACKAGE_NAME}","version":"{PI_PACKAGE_VERSION}","bin":{{"pi":"{PI_PACKAGE_BIN}"}}}}' > "$package/package.json"
  printf '#!/bin/sh\nprintf "%%s\\n" "{PI_PACKAGE_VERSION}"\n' > "$package/dist/cli.js"
  chmod 700 "$package/dist/cli.js"
  ln -s '../@earendil-works/pi-coding-agent/dist/cli.js' "$prefix/node_modules/.bin/pi"
  printf '%s\n' '{lock}' > "$prefix/package-lock.json"
  exit 0
fi
exit 9
"#
            ),
        )
        .expect("fake npm");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&npm, fs::Permissions::from_mode(0o700)).expect("npm executable");
        }
        let mut manager = ManagedExternalAgentDeploymentManager::new(
            Arc::new(Database::open_in_memory().expect("database")),
            temp.0.join("app-data/external-agents"),
            vec![npm],
        );
        manager.expected_package_integrity = format!(
            "sha512-{}",
            BASE64_STANDARD.encode(Sha512::digest(b"fixed archive"))
        );
        manager.expected_dependency_fingerprint =
            dependency_closure_fingerprint_value(&lock).expect("fixture closure");
        (temp, manager)
    }

    #[test]
    fn exact_pi_recipe_installs_privately_and_is_single_use() {
        let (_temp, manager) = fixture();
        let plan = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("plan");
        assert_eq!(plan.package_version, PI_PACKAGE_VERSION);
        assert!(
            plan.lifecycle_notes
                .iter()
                .any(|note| note.contains("PATH"))
        );
        let result = manager.apply_install(&plan.plan_id).expect("install");
        assert_eq!(result.managed_binary, manager.managed_pi_binary());
        assert!(result.managed_binary.is_file());
        assert!(matches!(
            manager.apply_install(&plan.plan_id),
            Err(ManagedExternalAgentDeploymentError::InvalidPlan)
        ));
        assert!(matches!(
            manager.plan_install(ExternalAgentIntegrationId::PiCodingAgent),
            Err(ManagedExternalAgentDeploymentError::AlreadyInstalled)
        ));
    }

    #[test]
    fn unsupported_recipe_and_registry_drift_fail_closed() {
        let (temp, manager) = fixture();
        assert!(matches!(
            manager.plan_install(ExternalAgentIntegrationId::OpenClaw),
            Err(ManagedExternalAgentDeploymentError::RecipeUnavailable)
        ));
        let npm = temp.0.join("bin/npm");
        fs::write(
            &npm,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 11.5.2; else echo '{\"name\":\"wrong\"}'; fi\n",
        )
        .expect("drifted npm");
        assert!(matches!(
            manager.plan_install(ExternalAgentIntegrationId::PiCodingAgent),
            Err(ManagedExternalAgentDeploymentError::PackageMetadataMismatch)
        ));
    }

    #[test]
    fn a_user_managed_pi_installation_blocks_the_private_recipe() {
        let (_temp, mut manager) = fixture();
        let user_pi = manager
            .managed_root
            .parent()
            .expect("fixture root")
            .join("user/pi");
        fs::create_dir_all(user_pi.parent().expect("user pi parent")).expect("user pi directory");
        fs::write(&user_pi, "#!/bin/sh\nexit 0\n").expect("user pi");
        manager.user_pi_candidates.push(user_pi);

        assert!(matches!(
            manager.plan_install(ExternalAgentIntegrationId::PiCodingAgent),
            Err(ManagedExternalAgentDeploymentError::AlreadyInstalled)
        ));
    }

    #[test]
    fn dependency_closure_drift_fails_before_runtime_activation() {
        let (_temp, mut manager) = fixture();
        let plan = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("plan");
        manager.expected_dependency_fingerprint = "0".repeat(64);

        assert!(matches!(
            manager.apply_install(&plan.plan_id),
            Err(ManagedExternalAgentDeploymentError::DependencyClosureMismatch)
        ));
        assert!(!manager.integration_runtime().exists());
    }

    #[test]
    fn private_runtime_removal_is_previewed_single_use_and_preserves_user_pi() {
        let (temp, mut manager) = fixture();
        let install = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("install plan");
        manager.apply_install(&install.plan_id).expect("install");
        let user_pi = temp.0.join("user/bin/pi");
        fs::create_dir_all(user_pi.parent().expect("user pi parent")).expect("user pi directory");
        fs::write(&user_pi, "#!/bin/sh\nexit 0\n").expect("user pi");
        manager.user_pi_candidates.push(user_pi.clone());

        let removal = manager
            .plan_removal(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("removal plan");
        assert!(
            removal
                .lifecycle_notes
                .iter()
                .any(|note| note.contains("用户自行安装"))
        );
        let result = manager
            .apply_removal_with_trash(&removal.plan_id, |path| {
                fs::remove_dir_all(path)?;
                Ok(())
            })
            .expect("remove private runtime");
        assert!(result.removed);
        assert!(result.moved_to_trash);
        assert!(result.user_installations_preserved);
        assert!(!manager.integration_runtime().exists());
        assert!(user_pi.is_file());
        assert!(matches!(
            manager.apply_removal_with_trash(&removal.plan_id, |_| Ok(())),
            Err(ManagedExternalAgentDeploymentError::InvalidPlan)
        ));
    }

    #[test]
    fn failed_trash_operation_restores_private_runtime() {
        let (_temp, manager) = fixture();
        let install = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("install plan");
        manager.apply_install(&install.plan_id).expect("install");
        let removal = manager
            .plan_removal(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("removal plan");

        assert!(matches!(
            manager.apply_removal_with_trash(&removal.plan_id, |_| {
                Err(ManagedExternalAgentDeploymentError::TrashFailed)
            }),
            Err(ManagedExternalAgentDeploymentError::TrashFailed)
        ));
        assert!(manager.integration_runtime().is_dir());
        assert!(manager.managed_pi_binary().is_file());
    }

    #[test]
    fn removal_revalidation_rejects_changed_dependency_lock() {
        let (_temp, manager) = fixture();
        let install = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("install plan");
        manager.apply_install(&install.plan_id).expect("install");
        let removal = manager
            .plan_removal(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("removal plan");
        fs::write(
            manager.integration_runtime().join("package-lock.json"),
            "{}",
        )
        .expect("tampered lock");

        assert!(matches!(
            manager.apply_removal_with_trash(&removal.plan_id, |_| Ok(())),
            Err(ManagedExternalAgentDeploymentError::ManagedInstallationChanged)
        ));
        assert!(manager.integration_runtime().is_dir());
    }

    #[test]
    fn removal_refuses_absent_private_runtime_even_when_user_pi_exists() {
        let (temp, mut manager) = fixture();
        let user_pi = temp.0.join("user/bin/pi");
        fs::create_dir_all(user_pi.parent().expect("user pi parent")).expect("user pi directory");
        fs::write(&user_pi, "#!/bin/sh\nexit 0\n").expect("user pi");
        manager.user_pi_candidates.push(user_pi.clone());

        assert!(matches!(
            manager.plan_removal(ExternalAgentIntegrationId::PiCodingAgent),
            Err(ManagedExternalAgentDeploymentError::ManagedInstallationNotFound)
        ));
        assert!(user_pi.is_file());
    }

    #[test]
    #[ignore = "requires a freshly resolved official npm package-lock"]
    fn official_pi_dependency_closure_matches_frozen_recipe() {
        let lock_path = std::env::var_os("HAL100_PI_PACKAGE_LOCK")
            .map(PathBuf::from)
            .expect("HAL100_PI_PACKAGE_LOCK must point to the resolved lockfile");
        assert_eq!(
            dependency_closure_fingerprint(&lock_path).expect("official dependency closure"),
            PI_DEPENDENCY_CLOSURE_SHA256
        );
    }

    #[test]
    #[ignore = "requires npm and the official registry"]
    fn official_pi_recipe_installs_and_previews_removal_in_isolation() {
        let npm = std::env::var_os("HAL100_NPM_BINARY")
            .map(PathBuf::from)
            .expect("HAL100_NPM_BINARY must point to npm");
        let temp = TestDirectory::new();
        let manager = ManagedExternalAgentDeploymentManager::new(
            Arc::new(Database::open_in_memory().expect("database")),
            temp.0.join("app-data/external-agents"),
            vec![npm],
        );
        let plan = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("official install plan");
        let installed = manager
            .apply_install(&plan.plan_id)
            .expect("official install");
        assert!(installed.managed_binary.is_file());
        assert_eq!(
            dependency_closure_fingerprint(
                &manager.integration_runtime().join("package-lock.json")
            )
            .expect("installed dependency closure"),
            PI_DEPENDENCY_CLOSURE_SHA256
        );
        manager
            .plan_removal(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("official removal preview");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_managed_root_is_rejected_without_writing_outside_scope() {
        use std::os::unix::fs::symlink;

        let (temp, manager) = fixture();
        let outside = temp.0.join("outside");
        fs::create_dir(&outside).expect("outside");
        fs::create_dir_all(manager.managed_root.parent().expect("managed parent"))
            .expect("managed parent");
        symlink(&outside, &manager.managed_root).expect("managed symlink");
        let plan = manager
            .plan_install(ExternalAgentIntegrationId::PiCodingAgent)
            .expect("plan before root validation");
        assert!(matches!(
            manager.apply_install(&plan.plan_id),
            Err(ManagedExternalAgentDeploymentError::UnsafeInstallRoot)
        ));
        assert!(
            fs::read_dir(&outside)
                .expect("outside contents")
                .next()
                .is_none()
        );
    }
}
