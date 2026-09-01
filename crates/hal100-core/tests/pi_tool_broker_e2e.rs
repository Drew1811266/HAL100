use std::{
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use hal100_core::SimulatedToolBroker;
use hal100_protocol::{
    AGENT_RPC_MAX_FRAME_BYTES, AGENT_RPC_VERSION, AgentRpcEnvelope, ToolCallRequestPayload,
    encode_agent_rpc_frame,
};
use serde_json::{Value, json};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// This is ignored by the default Rust test suite because it requires a built JavaScript Sidecar.
/// Run after `pnpm --filter @hal100/agent-kernel build` with:
/// `cargo test -p hal100-core --test pi_tool_broker_e2e -- --ignored`.
#[test]
#[ignore = "requires the built Agent Kernel Sidecar and the pinned Node runtime"]
fn rust_broker_completes_a_real_pi_tool_round_trip() {
    let workspace = workspace_root();
    let sidecar = workspace.join("sidecars/agent-kernel/dist/index.js");
    assert!(sidecar.is_file(), "build the Agent Kernel Sidecar first");

    let mut child = spawn_sidecar(&workspace, &sidecar);
    let mut child_stdin = child.stdin.take().expect("sidecar stdin");
    let mut child_stdout = child.stdout.take().expect("sidecar stdout");
    let (sender, receiver) = mpsc::channel();

    let reader = thread::spawn(move || {
        loop {
            match read_envelope(&mut child_stdout) {
                Ok(envelope) => {
                    if sender.send(envelope).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
                Err(error) => panic!("read Sidecar frame: {error}"),
            }
        }
    });

    write_envelope(
        &mut child_stdin,
        AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: "rust-e2e-simulation".to_owned(),
            kind: "agent.simulation.start".to_owned(),
            payload: json!({}),
        },
    );

    let broker = SimulatedToolBroker;
    let mut saw_tool_request = false;
    let mut saw_completion = false;

    loop {
        let envelope = match receiver.recv_timeout(RESPONSE_TIMEOUT) {
            Ok(envelope) => envelope,
            Err(error) => {
                let _ = child.kill();
                panic!("Sidecar response timed out: {error}");
            }
        };

        match envelope.kind.as_str() {
            "tool.call.request" => {
                let request: ToolCallRequestPayload =
                    serde_json::from_value(envelope.payload).expect("typed tool request");
                let result = broker.execute(&request);
                assert_eq!(result.tool_call_id, request.tool_call_id);
                saw_tool_request = true;

                write_envelope(
                    &mut child_stdin,
                    AgentRpcEnvelope {
                        protocol_version: AGENT_RPC_VERSION,
                        id: envelope.id,
                        kind: "tool.call.result".to_owned(),
                        payload: serde_json::to_value(result).expect("serialize broker result"),
                    },
                );
            }
            "agent.simulation.completed" => {
                assert_eq!(envelope.id, "rust-e2e-simulation");
                assert_eq!(envelope.payload["brokerRoundTrips"], 1);
                assert_eq!(envelope.payload["directSystemExecution"], false);
                assert_eq!(envelope.payload["modelRequests"], 0);
                assert_eq!(envelope.payload["networkRequests"], 0);
                saw_completion = true;

                write_envelope(
                    &mut child_stdin,
                    AgentRpcEnvelope {
                        protocol_version: AGENT_RPC_VERSION,
                        id: "rust-e2e-shutdown".to_owned(),
                        kind: "system.shutdown".to_owned(),
                        payload: json!({}),
                    },
                );
            }
            "system.shutdown.ack" => break,
            "system.error" => {
                let code = envelope
                    .payload
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                panic!("Sidecar returned system.error: {code}");
            }
            other => panic!("unexpected Sidecar message: {other}"),
        }
    }

    drop(child_stdin);
    let status = child.wait().expect("wait for Sidecar exit");
    reader.join().expect("join Sidecar frame reader");

    assert!(saw_tool_request);
    assert!(saw_completion);
    assert!(status.success());
}

#[test]
#[ignore = "iteration 9 repeated real Sidecar lifecycle probe"]
fn real_sidecar_starts_pings_and_exits_cleanly_twenty_five_times() {
    let workspace = workspace_root();
    let sidecar = workspace.join("sidecars/agent-kernel/dist/index.js");
    assert!(sidecar.is_file(), "build the Agent Kernel Sidecar first");
    let started = Instant::now();

    for cycle in 0..25 {
        let mut child = spawn_sidecar(&workspace, &sidecar);
        let mut stdin = child.stdin.take().expect("Sidecar stdin");
        let mut stdout = child.stdout.take().expect("Sidecar stdout");
        let ping_id = format!("stability-ping-{cycle}");
        write_envelope(
            &mut stdin,
            AgentRpcEnvelope {
                protocol_version: AGENT_RPC_VERSION,
                id: ping_id.clone(),
                kind: "system.ping".to_owned(),
                payload: json!({}),
            },
        );
        let pong = read_envelope(&mut stdout).expect("Sidecar pong");
        assert_eq!(pong.id, ping_id);
        assert_eq!(pong.kind, "system.pong");

        let shutdown_id = format!("stability-shutdown-{cycle}");
        write_envelope(
            &mut stdin,
            AgentRpcEnvelope {
                protocol_version: AGENT_RPC_VERSION,
                id: shutdown_id.clone(),
                kind: "system.shutdown".to_owned(),
                payload: json!({}),
            },
        );
        let acknowledgement = read_envelope(&mut stdout).expect("Sidecar shutdown acknowledgement");
        assert_eq!(acknowledgement.id, shutdown_id);
        assert_eq!(acknowledgement.kind, "system.shutdown.ack");
        drop(stdin);
        assert!(child.wait().expect("wait for Sidecar").success());
    }

    println!(
        "sidecar_lifecycle cycles=25 elapsed_ms={}",
        started.elapsed().as_millis()
    );
}

#[test]
#[ignore = "iteration 9 malformed RPC process-exit probe"]
fn oversized_rpc_frame_terminates_the_real_sidecar_without_hanging() {
    let workspace = workspace_root();
    let sidecar = workspace.join("sidecars/agent-kernel/dist/index.js");
    assert!(sidecar.is_file(), "build the Agent Kernel Sidecar first");
    let mut child = spawn_sidecar(&workspace, &sidecar);
    let mut stdin = child.stdin.take().expect("Sidecar stdin");
    let oversized = u32::try_from(AGENT_RPC_MAX_FRAME_BYTES + 1)
        .expect("RPC maximum fits u32")
        .to_be_bytes();
    stdin.write_all(&oversized).expect("write oversized prefix");
    stdin.flush().expect("flush oversized prefix");
    drop(stdin);

    let deadline = Instant::now() + RESPONSE_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll malformed Sidecar") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("malformed Sidecar did not exit within the bounded timeout");
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(2));
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn spawn_sidecar(workspace: &Path, sidecar: &Path) -> Child {
    #[cfg(not(windows))]
    let node = workspace.join("node_modules/node/bin/node");
    #[cfg(windows)]
    let node = workspace.join("node_modules/node/node.exe");
    assert!(node.is_file(), "install the pinned workspace Node runtime");
    Command::new(node)
        .arg(sidecar)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start Agent Kernel Sidecar")
}

fn write_envelope(stdin: &mut ChildStdin, envelope: AgentRpcEnvelope) {
    let frame = encode_agent_rpc_frame(&envelope).expect("encode Agent RPC frame");
    stdin.write_all(&frame).expect("write Sidecar frame");
    stdin.flush().expect("flush Sidecar frame");
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
