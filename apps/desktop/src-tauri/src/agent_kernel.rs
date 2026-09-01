use std::{
    env, fs,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use hal100_platform::{
    AgentKernelLaunchSpec, SidecarIsolation, SidecarLaunchError, prepare_agent_kernel_command,
};
use hal100_protocol::{
    AGENT_RPC_MAX_FRAME_BYTES, AGENT_RPC_VERSION, AgentRpcEnvelope, AgentRpcFrameError,
    encode_agent_rpc_frame,
};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

const PINNED_NODE_VERSION: &str = "v24.18.0";
const SIDECAR_RESPONSE_TIMEOUT: Duration = Duration::from_secs(180);
const SIDECAR_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const SIDECAR_CANCELLATION_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub(super) enum AgentKernelError {
    #[error("Agent Kernel is unavailable")]
    Unavailable,
    #[error("Agent Kernel runtime version does not match")]
    RuntimeVersion,
    #[error("Agent Kernel failed to start or exited unsuccessfully")]
    Start,
    #[error("Agent Kernel response timed out")]
    Timeout,
    #[error("Agent Kernel run was cancelled")]
    Cancelled,
    #[error("Agent Kernel protocol validation failed")]
    InvalidProtocol,
    #[error(transparent)]
    Launch(#[from] SidecarLaunchError),
    #[error(transparent)]
    Frame(#[from] AgentRpcFrameError),
    #[error("Agent Kernel local I/O failed")]
    Io(#[source] std::io::Error),
}

#[derive(Clone)]
pub(super) struct AgentKernelRunner {
    workspace_root: PathBuf,
    session_root: PathBuf,
    node_binary: PathBuf,
    entrypoint: PathBuf,
}

impl AgentKernelRunner {
    pub(super) fn discover(data_dir: &Path) -> Result<Self, AgentKernelError> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .map_err(|_| AgentKernelError::Unavailable)?;
        let entrypoint = workspace_root.join("sidecars/agent-kernel/dist/index.js");
        if !entrypoint.is_file() {
            return Err(AgentKernelError::Unavailable);
        }
        let node_binary = resolve_node_binary(&workspace_root)?;
        let session_root = data_dir.join("agent").join("sessions");
        fs::create_dir_all(&session_root).map_err(AgentKernelError::Io)?;
        set_owner_only_directory(&session_root).map_err(AgentKernelError::Io)?;
        Ok(Self {
            workspace_root,
            session_root,
            node_binary,
            entrypoint,
        })
    }

    pub(super) fn run<T, E>(
        &self,
        cancellation: &AtomicBool,
        exchange: impl FnOnce(&mut AgentKernelChannel) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<AgentKernelError>,
    {
        if cancellation.load(Ordering::Acquire) {
            return Err(E::from(AgentKernelError::Cancelled));
        }
        let session_directory = self
            .session_root
            .join(format!("session-{}", Uuid::new_v4().simple()));
        let _session = SessionDirectory::create(session_directory.clone()).map_err(E::from)?;
        let spec = AgentKernelLaunchSpec {
            runtime_binary: self.node_binary.clone(),
            entrypoint: self.entrypoint.clone(),
            working_directory: self.workspace_root.join("sidecars/agent-kernel"),
            workspace_root: self.workspace_root.clone(),
            session_root: session_directory,
            isolation: SidecarIsolation::ProcessBoundaryOnly,
            arguments: Vec::new(),
        };
        let mut command = prepare_agent_kernel_command(&spec)
            .map_err(AgentKernelError::from)
            .map_err(E::from)?;
        let mut child = ManagedChild::new(
            command
                .spawn()
                .map_err(|_| E::from(AgentKernelError::Start))?,
        );
        let stdin = child
            .child
            .stdin
            .take()
            .ok_or_else(|| E::from(AgentKernelError::Start))?;
        let stdout = child
            .child
            .stdout
            .take()
            .ok_or_else(|| E::from(AgentKernelError::Start))?;
        let stderr = child
            .child
            .stderr
            .take()
            .ok_or_else(|| E::from(AgentKernelError::Start))?;
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut stdout = stdout;
            loop {
                match read_envelope(&mut stdout) {
                    Ok(envelope) => {
                        if sender.send(Ok(envelope)).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let stderr_reader = thread::spawn(move || {
            let mut stderr = stderr;
            let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        });
        let mut channel = AgentKernelChannel { stdin, receiver };

        let exchange_result = exchange(&mut channel);
        drop(channel);
        if exchange_result.is_err() {
            child.terminate();
            let _ = reader.join();
            let _ = stderr_reader.join();
            return exchange_result;
        }
        let child_result = child.wait_for_exit();
        let _ = reader.join();
        let _ = stderr_reader.join();
        child_result.map_err(E::from)?;
        exchange_result
    }
}

pub(super) struct AgentKernelChannel {
    stdin: ChildStdin,
    receiver: mpsc::Receiver<Result<AgentRpcEnvelope, std::io::Error>>,
}

impl AgentKernelChannel {
    pub(super) fn send(&mut self, envelope: &AgentRpcEnvelope) -> Result<(), AgentKernelError> {
        validate_envelope(envelope)?;
        let frame = encode_agent_rpc_frame(envelope)?;
        self.stdin.write_all(&frame).map_err(AgentKernelError::Io)?;
        self.stdin.flush().map_err(AgentKernelError::Io)
    }

    pub(super) fn receive(
        &self,
        cancellation: &AtomicBool,
    ) -> Result<AgentRpcEnvelope, AgentKernelError> {
        receive_envelope_with_timeout(
            &self.receiver,
            cancellation,
            SIDECAR_RESPONSE_TIMEOUT,
            SIDECAR_CANCELLATION_POLL,
        )
    }

    pub(super) fn request_shutdown(
        &mut self,
        run_id: &str,
        cancellation: &AtomicBool,
    ) -> Result<(), AgentKernelError> {
        let shutdown_id = format!("shutdown-{run_id}");
        self.send(&AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: shutdown_id.clone(),
            kind: "system.shutdown".to_owned(),
            payload: json!({}),
        })?;
        let acknowledgement = self.receive(cancellation)?;
        if acknowledgement.id != shutdown_id || acknowledgement.kind != "system.shutdown.ack" {
            return Err(AgentKernelError::InvalidProtocol);
        }
        Ok(())
    }
}

fn receive_envelope_with_timeout(
    receiver: &mpsc::Receiver<Result<AgentRpcEnvelope, std::io::Error>>,
    cancellation: &AtomicBool,
    response_timeout: Duration,
    cancellation_poll: Duration,
) -> Result<AgentRpcEnvelope, AgentKernelError> {
    let started = Instant::now();
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(AgentKernelError::Cancelled);
        }
        let remaining = response_timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AgentKernelError::Timeout);
        }
        match receiver.recv_timeout(remaining.min(cancellation_poll)) {
            Ok(result) => {
                let envelope = result.map_err(AgentKernelError::Io)?;
                validate_envelope(&envelope)?;
                return Ok(envelope);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AgentKernelError::Io(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "HAL100 Agent Kernel closed its RPC stream",
                )));
            }
        }
    }
}

fn validate_envelope(envelope: &AgentRpcEnvelope) -> Result<(), AgentKernelError> {
    if envelope.protocol_version != AGENT_RPC_VERSION
        || envelope.id.is_empty()
        || envelope.id.len() > 128
    {
        return Err(AgentKernelError::InvalidProtocol);
    }
    Ok(())
}

fn read_envelope(reader: &mut impl Read) -> Result<AgentRpcEnvelope, std::io::Error> {
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix)?;
    let payload_length = u32::from_be_bytes(prefix) as usize;
    if payload_length > AGENT_RPC_MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "Agent RPC frame exceeds limit",
        ));
    }
    let mut payload = vec![0_u8; payload_length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))
}

fn resolve_node_binary(workspace_root: &Path) -> Result<PathBuf, AgentKernelError> {
    let candidate = env::var_os("HAL100_AGENT_NODE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("node_modules/node/bin/node"));
    let candidate = candidate
        .canonicalize()
        .map_err(|_| AgentKernelError::Unavailable)?;
    let output = Command::new(&candidate)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .map_err(|_| AgentKernelError::Unavailable)?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout).trim() != PINNED_NODE_VERSION
    {
        return Err(AgentKernelError::RuntimeVersion);
    }
    Ok(candidate)
}

fn set_owner_only_directory(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

struct SessionDirectory(PathBuf);

struct ManagedChild {
    child: Child,
    reaped: bool,
}

impl ManagedChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            reaped: false,
        }
    }

    fn wait_for_exit(&mut self) -> Result<(), AgentKernelError> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().map_err(AgentKernelError::Io)? {
                self.reaped = true;
                return status
                    .success()
                    .then_some(())
                    .ok_or(AgentKernelError::Start);
            }
            if started.elapsed() >= SIDECAR_EXIT_TIMEOUT {
                self.terminate();
                return Err(AgentKernelError::Timeout);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn terminate(&mut self) {
        if self.reaped {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.reaped = true;
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl SessionDirectory {
    fn create(path: PathBuf) -> Result<Self, AgentKernelError> {
        fs::create_dir_all(&path).map_err(AgentKernelError::Io)?;
        set_owner_only_directory(&path).map_err(AgentKernelError::Io)?;
        Ok(Self(path))
    }
}

impl Drop for SessionDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_observes_cancellation_without_waiting_for_the_response_timeout() {
        let (_sender, receiver) = mpsc::channel();
        let cancellation = AtomicBool::new(true);
        let started = Instant::now();
        assert!(matches!(
            receive_envelope_with_timeout(
                &receiver,
                &cancellation,
                Duration::from_secs(1),
                Duration::from_millis(10),
            ),
            Err(AgentKernelError::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn receive_bounds_silence_and_rejects_an_invalid_protocol_envelope() {
        let (sender, receiver) = mpsc::channel();
        let cancellation = AtomicBool::new(false);
        assert!(matches!(
            receive_envelope_with_timeout(
                &receiver,
                &cancellation,
                Duration::from_millis(5),
                Duration::from_millis(1),
            ),
            Err(AgentKernelError::Timeout)
        ));

        sender
            .send(Ok(AgentRpcEnvelope {
                protocol_version: AGENT_RPC_VERSION + 1,
                id: "invalid-version".to_owned(),
                kind: "system.pong".to_owned(),
                payload: json!({}),
            }))
            .expect("send invalid envelope");
        assert!(matches!(
            receive_envelope_with_timeout(
                &receiver,
                &cancellation,
                Duration::from_secs(1),
                Duration::from_millis(10),
            ),
            Err(AgentKernelError::InvalidProtocol)
        ));
    }

    #[test]
    fn session_directory_is_owner_only_and_removed_on_drop() {
        let parent = env::temp_dir().join(format!(
            "hal100-agent-kernel-session-test-{}",
            Uuid::new_v4().simple()
        ));
        let session_path = parent.join("session");
        {
            let _session = SessionDirectory::create(session_path.clone()).expect("create session");
            assert!(session_path.is_dir());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&session_path)
                    .expect("session metadata")
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o700);
            }
        }
        assert!(!session_path.exists());
        let _ = fs::remove_dir_all(parent);
    }
}
