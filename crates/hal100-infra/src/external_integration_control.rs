use std::{
    collections::HashMap,
    ffi::OsString,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, RwLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use thiserror::Error;
use uuid::Uuid;

use hal100_protocol::{ExternalAgentInputModality, ExternalAgentModelProfile};

use crate::{
    AgentRuntimeCapacityProfile, MANAGED_ROUTE_MAX_OUTPUT_TOKENS, MANAGED_ROUTE_PROFILE_REVISION,
};

const SANITIZED_EXEC_PATH: &str = "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin";

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PendingPlanError {
    #[error("integration plan store lock was poisoned")]
    LockPoisoned,
    #[error("integration plan does not exist, was consumed, or expired")]
    InvalidPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPlanTicket {
    pub plan_id: String,
    pub expires_at_ms: i64,
}

struct PendingPlanEntry<T> {
    expires_at_ms: i64,
    value: T,
}

/// One-use, bounded-lifetime plan storage shared by external integration adapters.
///
/// Each adapter owns a separate instance, so replacing a Pi preview can never consume an
/// OpenCode or Hermes preview. Replacing a preview immediately drops any transient plaintext
/// credential held by the older value.
pub struct PendingPlanStore<T> {
    ttl: Duration,
    entries: Mutex<HashMap<String, PendingPlanEntry<T>>>,
}

impl<T> PendingPlanStore<T> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn replace(&self, value: T) -> Result<PendingPlanTicket, PendingPlanError> {
        self.replace_at(value, now_ms())
    }

    pub fn take(&self, plan_id: &str) -> Result<T, PendingPlanError> {
        self.take_at(plan_id, now_ms())
    }

    pub fn discard(&self, plan_id: &str) -> Result<bool, PendingPlanError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PendingPlanError::LockPoisoned)?;
        Ok(entries.remove(plan_id).is_some())
    }

    fn replace_at(
        &self,
        value: T,
        created_at_ms: i64,
    ) -> Result<PendingPlanTicket, PendingPlanError> {
        let expires_at_ms =
            created_at_ms.saturating_add(i64::try_from(self.ttl.as_millis()).unwrap_or(i64::MAX));
        let plan_id = Uuid::new_v4().to_string();
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PendingPlanError::LockPoisoned)?;
        entries.clear();
        entries.insert(
            plan_id.clone(),
            PendingPlanEntry {
                expires_at_ms,
                value,
            },
        );
        Ok(PendingPlanTicket {
            plan_id,
            expires_at_ms,
        })
    }

    fn take_at(&self, plan_id: &str, now_ms: i64) -> Result<T, PendingPlanError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PendingPlanError::LockPoisoned)?;
        let entry = entries
            .remove(plan_id)
            .ok_or(PendingPlanError::InvalidPlan)?;
        if entry.expires_at_ms < now_ms {
            return Err(PendingPlanError::InvalidPlan);
        }
        Ok(entry.value)
    }
}

impl<T: Clone> PendingPlanStore<T> {
    pub fn peek(&self, plan_id: &str) -> Result<T, PendingPlanError> {
        let now_ms = now_ms();
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PendingPlanError::LockPoisoned)?;
        entries.retain(|_, entry| entry.expires_at_ms >= now_ms);
        entries
            .get(plan_id)
            .map(|entry| entry.value.clone())
            .ok_or(PendingPlanError::InvalidPlan)
    }
}

#[derive(Debug, Error)]
pub enum BoundedCommandError {
    #[error("failed to start external client command: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("external client command timed out")]
    TimedOut,
    #[error("external client command failed")]
    Failed,
    #[error("external client command output exceeded its safety limit")]
    OutputTooLarge,
    #[error("external client command output was not UTF-8")]
    InvalidUtf8,
    #[error("external client command output reader failed")]
    ReaderFailed,
}

#[derive(Debug, Clone)]
pub struct BoundedCommandRunner {
    timeout: Duration,
    max_stdout_bytes: usize,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ModelProfileError {
    #[error("external Agent model profile is invalid")]
    InvalidProfile,
    #[error("external Agent model profile registry is unavailable")]
    RegistryUnavailable,
}

#[derive(Clone)]
pub struct ExternalModelProfileRegistry {
    profile: Arc<RwLock<ExternalAgentModelProfile>>,
}

impl ExternalModelProfileRegistry {
    pub fn new(profile: ExternalAgentModelProfile) -> Result<Self, ModelProfileError> {
        validate_model_profile(&profile)?;
        Ok(Self {
            profile: Arc::new(RwLock::new(profile)),
        })
    }

    /// Conservative profile for the current HAL100-managed llama.cpp route.
    ///
    /// Adapters must refresh this value when route-specific capability metadata becomes
    /// available instead of silently inheriting an upstream client's optimistic defaults.
    pub fn managed_route(capacity: AgentRuntimeCapacityProfile) -> Self {
        Self::new(ExternalAgentModelProfile {
            model_id: "hal100-active".to_owned(),
            display_name: "HAL100 当前模型".to_owned(),
            context_window_tokens: capacity.context_window_tokens,
            max_output_tokens: MANAGED_ROUTE_MAX_OUTPUT_TOKENS,
            input_modalities: vec![ExternalAgentInputModality::Text],
            supports_tools: true,
            supports_reasoning: false,
            revision: MANAGED_ROUTE_PROFILE_REVISION.to_owned(),
        })
        .expect("the built-in conservative profile is valid")
    }

    /// Conservative test/default profile. Product startup always supplies the Rust-selected
    /// device profile through [`Self::managed_route`].
    pub fn conservative_managed_route() -> Self {
        Self::managed_route(AgentRuntimeCapacityProfile::baseline())
    }

    pub fn snapshot(&self) -> Result<ExternalAgentModelProfile, ModelProfileError> {
        self.profile
            .read()
            .map(|profile| profile.clone())
            .map_err(|_| ModelProfileError::RegistryUnavailable)
    }

    pub fn replace(&self, profile: ExternalAgentModelProfile) -> Result<(), ModelProfileError> {
        validate_model_profile(&profile)?;
        *self
            .profile
            .write()
            .map_err(|_| ModelProfileError::RegistryUnavailable)? = profile;
        Ok(())
    }
}

fn validate_model_profile(profile: &ExternalAgentModelProfile) -> Result<(), ModelProfileError> {
    let text_count = profile
        .input_modalities
        .iter()
        .filter(|modality| **modality == ExternalAgentInputModality::Text)
        .count();
    let unique_modalities = profile
        .input_modalities
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    if profile.model_id != "hal100-active"
        || profile.display_name.trim().is_empty()
        || profile.context_window_tokens == 0
        || profile.max_output_tokens == 0
        || profile.max_output_tokens >= profile.context_window_tokens
        || text_count != 1
        || unique_modalities.len() != profile.input_modalities.len()
        || profile.revision.trim().is_empty()
    {
        return Err(ModelProfileError::InvalidProfile);
    }
    Ok(())
}

impl BoundedCommandRunner {
    pub fn new(timeout: Duration, max_stdout_bytes: usize) -> Self {
        Self {
            timeout,
            max_stdout_bytes,
        }
    }

    pub fn run_utf8(&self, binary: &Path, args: &[&str]) -> Result<String, BoundedCommandError> {
        self.run_utf8_with_env(binary, args, &[])
    }

    /// Runs an external client command without inheriting the desktop process environment.
    ///
    /// Callers must explicitly pass every client-specific variable. PATH is always replaced by
    /// HAL100's small executable-search path, so an adapter cannot accidentally expose API keys,
    /// HOME/profile overrides, or unrelated application state to an upstream CLI.
    pub fn run_utf8_with_env(
        &self,
        binary: &Path,
        args: &[&str],
        environment: &[(String, OsString)],
    ) -> Result<String, BoundedCommandError> {
        self.run_utf8_with_env_in_dir(binary, args, environment, None)
    }

    /// Runs a bounded command from an explicitly selected working directory.
    ///
    /// Package managers and other developer tools commonly inspect configuration files in the
    /// current project. Deployment callers use this entry point to isolate those tools inside a
    /// HAL100-owned staging directory instead of inheriting the desktop process working tree.
    pub fn run_utf8_with_env_in_dir(
        &self,
        binary: &Path,
        args: &[&str],
        environment: &[(String, OsString)],
        current_directory: Option<&Path>,
    ) -> Result<String, BoundedCommandError> {
        let mut command = Command::new(binary);
        command
            .args(args)
            .env_clear()
            .env("PATH", SANITIZED_EXEC_PATH)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(current_directory) = current_directory {
            command.current_dir(current_directory);
        }
        for (name, value) in environment {
            command.env(name, value);
        }
        let mut child = command.spawn().map_err(BoundedCommandError::Spawn)?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or(BoundedCommandError::ReaderFailed)?;
        let max_stdout_bytes = self.max_stdout_bytes;
        let reader = thread::spawn(move || -> Result<(Vec<u8>, bool), std::io::Error> {
            let mut retained = Vec::with_capacity(max_stdout_bytes.min(8 * 1024));
            let mut exceeded = false;
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stdout.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                let remaining = max_stdout_bytes.saturating_sub(retained.len());
                retained.extend_from_slice(&buffer[..read.min(remaining)]);
                exceeded |= read > remaining;
            }
            Ok((retained, exceeded))
        });

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(BoundedCommandError::Spawn)? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(BoundedCommandError::TimedOut);
            }
            thread::sleep(Duration::from_millis(20));
        };
        let (stdout, exceeded) = reader
            .join()
            .map_err(|_| BoundedCommandError::ReaderFailed)?
            .map_err(|_| BoundedCommandError::ReaderFailed)?;
        if !status.success() {
            return Err(BoundedCommandError::Failed);
        }
        if exceeded {
            return Err(BoundedCommandError::OutputTooLarge);
        }
        String::from_utf8(stdout).map_err(|_| BoundedCommandError::InvalidUtf8)
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

    #[test]
    fn plans_are_single_use_scoped_and_expire() {
        let plans = PendingPlanStore::new(Duration::from_millis(50));
        let first = plans.replace_at("first", 100).expect("first plan");
        let second = plans.replace_at("second", 110).expect("second plan");

        assert_eq!(
            plans.take_at(&first.plan_id, 111),
            Err(PendingPlanError::InvalidPlan)
        );
        assert_eq!(
            plans.take_at(&second.plan_id, 161),
            Err(PendingPlanError::InvalidPlan)
        );

        let third = plans.replace_at("third", 200).expect("third plan");
        assert_eq!(plans.take_at(&third.plan_id, 250), Ok("third"));
        assert_eq!(
            plans.take_at(&third.plan_id, 250),
            Err(PendingPlanError::InvalidPlan)
        );

        let live = plans.replace("live").expect("live plan");
        assert_eq!(plans.peek(&live.plan_id), Ok("live"));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_runner_clears_environment_and_limits_output() {
        let runner = BoundedCommandRunner::new(Duration::from_secs(1), 5);
        assert_eq!(
            runner
                .run_utf8(Path::new("/usr/bin/printf"), &["hello"])
                .expect("bounded output"),
            "hello"
        );
        assert!(matches!(
            runner.run_utf8(Path::new("/usr/bin/printf"), &["too-long"]),
            Err(BoundedCommandError::OutputTooLarge)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_runner_exposes_only_explicit_client_environment() {
        let runner = BoundedCommandRunner::new(Duration::from_secs(1), 64);
        let environment = [("HAL100_ALLOWED_TEST".to_owned(), OsString::from("present"))];

        assert_eq!(
            runner
                .run_utf8_with_env(
                    Path::new("/bin/sh"),
                    &[
                        "-c",
                        "test -z \"$HOME\" && printf %s \"$HAL100_ALLOWED_TEST\""
                    ],
                    &environment,
                )
                .expect("sanitized environment"),
            "present"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_runner_terminates_a_hung_process() {
        let runner = BoundedCommandRunner::new(Duration::from_millis(20), 64);
        assert!(matches!(
            runner.run_utf8(Path::new("/bin/sleep"), &["2"]),
            Err(BoundedCommandError::TimedOut)
        ));
    }

    #[test]
    fn model_profiles_are_explicit_validated_and_hot_replaceable() {
        let registry = ExternalModelProfileRegistry::conservative_managed_route();
        assert_eq!(
            registry
                .snapshot()
                .expect("initial profile")
                .context_window_tokens,
            crate::AGENT_BASELINE_CONTEXT_WINDOW_TOKENS
        );
        let invalid = ExternalAgentModelProfile {
            model_id: "hal100-active".to_owned(),
            display_name: "Invalid".to_owned(),
            context_window_tokens: 1_024,
            max_output_tokens: 1_024,
            input_modalities: vec![ExternalAgentInputModality::Text],
            supports_tools: true,
            supports_reasoning: false,
            revision: "invalid".to_owned(),
        };
        assert!(matches!(
            registry.replace(invalid),
            Err(ModelProfileError::InvalidProfile)
        ));

        let mut updated = registry.snapshot().expect("profile");
        updated.context_window_tokens = 32_768;
        updated.max_output_tokens = 4_096;
        updated.revision = "external-route-2".to_owned();
        registry.replace(updated).expect("replace profile");
        assert_eq!(
            registry.snapshot().expect("updated profile").revision,
            "external-route-2"
        );
    }
}
