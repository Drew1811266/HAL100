#![cfg(target_os = "macos")]

use std::{
    ffi::OsString,
    fs,
    io::{ErrorKind, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{ChildStdin, Command},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

use hal100_core::SimulatedToolBroker;
use hal100_platform::{AgentKernelLaunchSpec, SidecarIsolation, prepare_agent_kernel_command};
use hal100_protocol::{
    AGENT_RPC_MAX_FRAME_BYTES, AGENT_RPC_VERSION, AgentRpcEnvelope, ToolCallRequestPayload,
    encode_agent_rpc_frame,
};
use serde_json::{Value, json};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
#[ignore = "uses deprecated sandbox-exec as an unsigned-development regression probe"]
fn development_sandbox_runs_rpc_and_denies_files_network_and_parent_environment() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let node_binary = discover_pinned_node(workspace);
    let test_root = TestRoot::new();
    let profile = workspace.join("sidecars/agent-kernel/sandbox/macos-development.sb");

    run_agent_kernel_ping(
        workspace,
        &node_binary,
        &profile,
        test_root.path().join("agent-session"),
    );
    run_denial_probe(
        workspace,
        &node_binary,
        &profile,
        test_root.path().join("probe-session"),
        test_root.path(),
    );
}

fn run_agent_kernel_ping(
    workspace: &Path,
    node_binary: &Path,
    profile: &Path,
    session_root: PathBuf,
) {
    let spec = AgentKernelLaunchSpec {
        runtime_binary: node_binary.to_owned(),
        entrypoint: workspace.join("sidecars/agent-kernel/dist/index.js"),
        working_directory: workspace.join("sidecars/agent-kernel"),
        workspace_root: workspace.to_owned(),
        session_root,
        isolation: SidecarIsolation::MacOsDevelopmentSandbox {
            profile: profile.to_owned(),
        },
        arguments: Vec::new(),
    };
    let mut child = prepare_agent_kernel_command(&spec)
        .expect("prepare sandboxed Agent Kernel")
        .spawn()
        .expect("spawn sandboxed Agent Kernel");
    let mut stdin = child.stdin.take().expect("Agent Kernel stdin");
    let mut stdout = child.stdout.take().expect("Agent Kernel stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        loop {
            match read_envelope(&mut stdout) {
                Ok(envelope) => {
                    if sender.send(envelope).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
                Err(error) => panic!("read sandboxed Sidecar frame: {error}"),
            }
        }
    });

    write_envelope(
        &mut stdin,
        AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: "sandbox-ping".to_owned(),
            kind: "system.ping".to_owned(),
            payload: json!({}),
        },
    );
    let pong = receive_or_kill(&receiver, &mut child);
    assert_eq!(pong.kind, "system.pong");
    assert_eq!(pong.payload["directToolExecutionEnabled"], false);

    write_envelope(
        &mut stdin,
        AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: "sandbox-simulation".to_owned(),
            kind: "agent.simulation.start".to_owned(),
            payload: json!({}),
        },
    );
    let tool_request = receive_or_kill(&receiver, &mut child);
    assert_eq!(tool_request.kind, "tool.call.request");
    let request: ToolCallRequestPayload =
        serde_json::from_value(tool_request.payload).expect("typed sandboxed tool request");
    let broker_result = SimulatedToolBroker.execute(&request);
    write_envelope(
        &mut stdin,
        AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: tool_request.id,
            kind: "tool.call.result".to_owned(),
            payload: serde_json::to_value(broker_result).expect("serialize broker result"),
        },
    );
    let simulation = receive_or_kill(&receiver, &mut child);
    assert_eq!(simulation.kind, "agent.simulation.completed");
    assert_eq!(simulation.payload["brokerRoundTrips"], 1);
    assert_eq!(simulation.payload["directSystemExecution"], false);

    write_envelope(
        &mut stdin,
        AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: "sandbox-shutdown".to_owned(),
            kind: "system.shutdown".to_owned(),
            payload: json!({}),
        },
    );
    let shutdown = receive_or_kill(&receiver, &mut child);
    assert_eq!(shutdown.kind, "system.shutdown.ack");

    drop(stdin);
    assert!(child.wait().expect("wait for sandboxed Sidecar").success());
    reader.join().expect("join sandboxed Sidecar reader");
}

fn run_denial_probe(
    workspace: &Path,
    node_binary: &Path,
    profile: &Path,
    session_root: PathBuf,
    test_root: &Path,
) {
    let denied_root = test_root.join("denied");
    fs::create_dir_all(&denied_root).expect("create denied probe root");
    let denied_read = denied_root.join("secret.txt");
    let denied_write = denied_root.join("unexpected.txt");
    fs::write(&denied_read, "sandbox-marker").expect("write probe marker");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback probe listener");
    let port = listener
        .local_addr()
        .expect("probe listener address")
        .port();

    let spec = AgentKernelLaunchSpec {
        runtime_binary: node_binary.to_owned(),
        entrypoint: workspace.join("sidecars/agent-kernel/sandbox/probe.mjs"),
        working_directory: workspace.join("sidecars/agent-kernel"),
        workspace_root: workspace.to_owned(),
        session_root,
        isolation: SidecarIsolation::MacOsDevelopmentSandbox {
            profile: profile.to_owned(),
        },
        arguments: vec![
            denied_read.into_os_string(),
            denied_write.clone().into_os_string(),
            OsString::from(port.to_string()),
        ],
    };

    let output = prepare_agent_kernel_command(&spec)
        .expect("prepare sandbox denial probe")
        .output()
        .expect("run sandbox denial probe");
    drop(listener);
    assert!(
        output.status.success(),
        "sandbox probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("sandbox probe JSON");

    for field in [
        "readDenied",
        "writeDenied",
        "networkDenied",
        "processDenied",
        "inheritedEnvironmentAbsent",
        "isolatedDirectories",
    ] {
        assert_eq!(result[field], true, "sandbox probe failed field {field}");
    }
    assert!(!denied_write.exists());
}

fn receive_or_kill(
    receiver: &mpsc::Receiver<AgentRpcEnvelope>,
    child: &mut std::process::Child,
) -> AgentRpcEnvelope {
    match receiver.recv_timeout(RESPONSE_TIMEOUT) {
        Ok(envelope) => envelope,
        Err(error) => {
            let _ = child.kill();
            panic!("sandboxed Sidecar response timed out: {error}");
        }
    }
}

fn discover_pinned_node(workspace: &Path) -> PathBuf {
    let output = Command::new("pnpm")
        .args(["exec", "node", "-p", "process.execPath"])
        .current_dir(workspace)
        .output()
        .expect("locate pinned Node runtime");
    assert!(output.status.success());
    PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("Node path is UTF-8")
            .trim(),
    )
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "hal100-sandbox-e2e-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("current time")
                .as_nanos()
        )))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn write_envelope(stdin: &mut ChildStdin, envelope: AgentRpcEnvelope) {
    let frame = encode_agent_rpc_frame(&envelope).expect("encode Agent RPC frame");
    stdin.write_all(&frame).expect("write Agent RPC frame");
    stdin.flush().expect("flush Agent RPC frame");
}

fn read_envelope(reader: &mut impl Read) -> std::io::Result<AgentRpcEnvelope> {
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix)?;
    let payload_length = u32::from_be_bytes(prefix) as usize;
    if payload_length > AGENT_RPC_MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "Sidecar frame exceeds the Agent RPC limit",
        ));
    }

    let mut payload = vec![0_u8; payload_length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))
}
