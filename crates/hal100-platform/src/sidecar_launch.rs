use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarIsolation {
    /// Portable process boundary used until a supported platform sandbox is available.
    ProcessBoundaryOnly,
    /// Unsigned-development regression probe. `sandbox-exec` is deprecated and is
    /// never considered a release security boundary.
    MacOsDevelopmentSandbox { profile: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentKernelLaunchSpec {
    pub runtime_binary: PathBuf,
    pub entrypoint: PathBuf,
    pub working_directory: PathBuf,
    pub workspace_root: PathBuf,
    pub session_root: PathBuf,
    pub isolation: SidecarIsolation,
    pub arguments: Vec<OsString>,
}

#[derive(Debug, Error)]
pub enum SidecarLaunchError {
    #[error("Agent Kernel runtime binary is unavailable")]
    RuntimeBinary(#[source] std::io::Error),
    #[error("Agent Kernel entrypoint is unavailable")]
    Entrypoint(#[source] std::io::Error),
    #[error("Agent Kernel working directory is unavailable")]
    WorkingDirectory(#[source] std::io::Error),
    #[error("Agent Kernel workspace root is unavailable")]
    WorkspaceRoot(#[source] std::io::Error),
    #[error("Agent Kernel files must remain inside the configured workspace root")]
    OutsideWorkspace,
    #[error("failed to create the isolated Agent Kernel session directory")]
    SessionDirectory(#[source] std::io::Error),
    #[error("the development sandbox profile is unavailable")]
    SandboxProfile(#[source] std::io::Error),
    #[error("the requested Sidecar isolation mode is unsupported on this platform")]
    UnsupportedIsolation,
}

pub fn prepare_agent_kernel_command(
    spec: &AgentKernelLaunchSpec,
) -> Result<Command, SidecarLaunchError> {
    let runtime_binary =
        fs::canonicalize(&spec.runtime_binary).map_err(SidecarLaunchError::RuntimeBinary)?;
    let entrypoint = fs::canonicalize(&spec.entrypoint).map_err(SidecarLaunchError::Entrypoint)?;
    let working_directory =
        fs::canonicalize(&spec.working_directory).map_err(SidecarLaunchError::WorkingDirectory)?;
    let workspace_root =
        fs::canonicalize(&spec.workspace_root).map_err(SidecarLaunchError::WorkspaceRoot)?;

    if !entrypoint.starts_with(&workspace_root) || !working_directory.starts_with(&workspace_root) {
        return Err(SidecarLaunchError::OutsideWorkspace);
    }

    let home_directory = spec.session_root.join("home");
    let temp_directory = spec.session_root.join("tmp");
    fs::create_dir_all(&home_directory).map_err(SidecarLaunchError::SessionDirectory)?;
    fs::create_dir_all(&temp_directory).map_err(SidecarLaunchError::SessionDirectory)?;
    let session_root =
        fs::canonicalize(&spec.session_root).map_err(SidecarLaunchError::SessionDirectory)?;
    let home_directory = session_root.join("home");
    let temp_directory = session_root.join("tmp");

    let mut command = match &spec.isolation {
        SidecarIsolation::ProcessBoundaryOnly => Command::new(&runtime_binary),
        SidecarIsolation::MacOsDevelopmentSandbox { profile } => {
            development_sandbox_command(profile, &runtime_binary, &workspace_root, &session_root)?
        }
    };

    command
        .arg(&entrypoint)
        .args(&spec.arguments)
        .current_dir(&working_directory)
        .env_clear()
        .env("HOME", home_directory)
        .env("TMPDIR", temp_directory)
        .env("LANG", "en_US.UTF-8")
        .env("NO_COLOR", "1")
        .env("NODE_NO_WARNINGS", "1")
        .env("HAL100_RPC_VERSION", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    Ok(command)
}

#[cfg(target_os = "macos")]
fn development_sandbox_command(
    profile: &Path,
    runtime_binary: &Path,
    workspace_root: &Path,
    session_root: &Path,
) -> Result<Command, SidecarLaunchError> {
    let profile = fs::canonicalize(profile).map_err(SidecarLaunchError::SandboxProfile)?;
    if !Path::new("/usr/bin/sandbox-exec").is_file() {
        return Err(SidecarLaunchError::UnsupportedIsolation);
    }
    let runtime_root = runtime_binary
        .parent()
        .and_then(Path::parent)
        .ok_or(SidecarLaunchError::UnsupportedIsolation)?;

    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-f")
        .arg(profile)
        .arg("-D")
        .arg(profile_definition("NODE_BINARY", runtime_binary))
        .arg("-D")
        .arg(profile_definition("NODE_ROOT", runtime_root))
        .arg("-D")
        .arg(profile_definition("WORKSPACE_ROOT", workspace_root))
        .arg("-D")
        .arg(profile_definition("SESSION_ROOT", session_root))
        .arg(runtime_binary);
    Ok(command)
}

#[cfg(not(target_os = "macos"))]
fn development_sandbox_command(
    _profile: &Path,
    _runtime_binary: &Path,
    _workspace_root: &Path,
    _session_root: &Path,
) -> Result<Command, SidecarLaunchError> {
    Err(SidecarLaunchError::UnsupportedIsolation)
}

#[cfg(target_os = "macos")]
fn profile_definition(key: &str, value: &Path) -> OsString {
    let mut definition = OsString::from(key);
    definition.push("=");
    definition.push(value.as_os_str());
    definition
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::SystemTime};

    use super::*;

    #[test]
    fn clears_the_parent_environment_and_uses_a_fake_home() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let session_root = std::env::temp_dir().join(format!(
            "hal100-sidecar-launch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("current time")
                .as_nanos()
        ));
        let spec = AgentKernelLaunchSpec {
            runtime_binary: std::env::current_exe().expect("test executable"),
            entrypoint: workspace.join("sidecars/agent-kernel/src/index.ts"),
            working_directory: workspace.join("sidecars/agent-kernel"),
            workspace_root: workspace.to_owned(),
            session_root: session_root.clone(),
            isolation: SidecarIsolation::ProcessBoundaryOnly,
            arguments: Vec::new(),
        };

        let command = prepare_agent_kernel_command(&spec).expect("prepared command");
        let canonical_session_root =
            fs::canonicalize(&session_root).expect("canonical session root");
        let environment: BTreeMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned)))
            .collect();

        assert_eq!(environment.len(), 6);
        assert!(!environment.contains_key(std::ffi::OsStr::new("PATH")));
        assert!(!environment.contains_key(std::ffi::OsStr::new("SSH_AUTH_SOCK")));
        assert_eq!(
            environment
                .get(std::ffi::OsStr::new("HOME"))
                .and_then(Option::as_deref),
            Some(canonical_session_root.join("home").as_os_str())
        );
        assert_eq!(
            command.get_current_dir(),
            Some(spec.working_directory.as_path())
        );

        fs::remove_dir_all(session_root).expect("remove test session root");
    }

    #[test]
    fn an_official_pi_installation_remains_outside_the_agent_kernel_scope() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let test_root = std::env::temp_dir().join(format!(
            "hal100-pi-coexistence-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("current time")
                .as_nanos()
        ));
        let user_home = test_root.join("user-home");
        let official_pi_directory = user_home.join(".pi/agent");
        let official_pi_settings = official_pi_directory.join("settings.json");
        let official_pi_binary = user_home.join(".local/bin/pi");
        fs::create_dir_all(&official_pi_directory).expect("official Pi configuration directory");
        fs::create_dir_all(
            official_pi_binary
                .parent()
                .expect("official Pi binary parent"),
        )
        .expect("official Pi binary directory");
        fs::write(
            &official_pi_settings,
            br#"{"defaultProvider":"user-provider","defaultModel":"user-model"}"#,
        )
        .expect("official Pi settings");
        fs::write(&official_pi_binary, b"official-pi-placeholder")
            .expect("official Pi binary placeholder");
        let original_settings = fs::read(&official_pi_settings).expect("settings before launch");

        let session_root = test_root.join("hal100-session");
        let spec = AgentKernelLaunchSpec {
            runtime_binary: std::env::current_exe().expect("test executable"),
            entrypoint: workspace.join("sidecars/agent-kernel/src/index.ts"),
            working_directory: workspace.join("sidecars/agent-kernel"),
            workspace_root: workspace.to_owned(),
            session_root: session_root.clone(),
            isolation: SidecarIsolation::ProcessBoundaryOnly,
            arguments: Vec::new(),
        };

        let command = prepare_agent_kernel_command(&spec).expect("prepared command");
        let environment: BTreeMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned)))
            .collect();
        let hal100_home = fs::canonicalize(&session_root)
            .expect("canonical session root")
            .join("home");

        assert_eq!(
            environment
                .get(std::ffi::OsStr::new("HOME"))
                .and_then(Option::as_deref),
            Some(hal100_home.as_os_str())
        );
        assert_ne!(hal100_home, user_home);
        assert!(!environment.contains_key(std::ffi::OsStr::new("PATH")));
        assert!(!environment.contains_key(std::ffi::OsStr::new("PI_CODING_AGENT_DIR")));
        assert!(!environment.contains_key(std::ffi::OsStr::new("PI_CODING_AGENT_SESSION_DIR")));
        assert_ne!(command.get_program(), official_pi_binary.as_os_str());
        assert_eq!(
            fs::read(&official_pi_settings).expect("settings after launch preparation"),
            original_settings
        );

        fs::remove_dir_all(test_root).expect("remove Pi coexistence test root");
    }

    #[test]
    fn rejects_an_entrypoint_outside_the_workspace() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let spec = AgentKernelLaunchSpec {
            runtime_binary: std::env::current_exe().expect("test executable"),
            entrypoint: PathBuf::from("/System/Library/CoreServices/SystemVersion.plist"),
            working_directory: workspace.join("sidecars/agent-kernel"),
            workspace_root: workspace.to_owned(),
            session_root: std::env::temp_dir().join("hal100-never-created"),
            isolation: SidecarIsolation::ProcessBoundaryOnly,
            arguments: Vec::new(),
        };

        assert!(matches!(
            prepare_agent_kernel_command(&spec),
            Err(SidecarLaunchError::OutsideWorkspace)
        ));
    }
}
