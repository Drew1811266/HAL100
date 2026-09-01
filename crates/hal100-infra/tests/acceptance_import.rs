mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use hal100_infra::{
    ExternalInferenceEngineRegistry, InferenceEngineAcceptanceEvidence,
    InferenceEngineAcceptanceLedger, InferenceEngineAcceptanceResilience,
    InferenceEngineAcceptanceRun, InferenceEngineAcceptanceRunOutcome,
    InferenceEngineAcceptanceStability,
};
use hal100_protocol::{
    InferenceEngineKind, InferenceEngineSupportEvidenceSummary, InferenceEngineSupportStatus,
};
use uuid::Uuid;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hal100-acceptance-import-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).expect("create acceptance-import temp directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn acceptance_import_binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_hal100_engine_acceptance_import")
        .or(option_env!("CARGO_BIN_EXE_hal100-engine-acceptance-import"))
        .map(PathBuf::from)
        .expect("cargo must expose the acceptance import binary to integration tests")
}

fn run_import(run: &Path, ledger: &Path, output: &Path, model_revision: &str) -> Output {
    Command::new(acceptance_import_binary())
        .args([
            "--run",
            run.to_str().expect("run path is UTF-8"),
            "--ledger",
            ledger.to_str().expect("ledger path is UTF-8"),
            "--output",
            output.to_str().expect("output path is UTF-8"),
            "--model-revision",
            model_revision,
        ])
        .output()
        .expect("run acceptance import binary")
}

fn run_requalification_import(
    run: &Path,
    ledger: &Path,
    output: &Path,
    model_revision: &str,
    existing_record_id: &str,
) -> Output {
    Command::new(acceptance_import_binary())
        .args([
            "--run",
            run.to_str().expect("run path is UTF-8"),
            "--ledger",
            ledger.to_str().expect("ledger path is UTF-8"),
            "--output",
            output.to_str().expect("output path is UTF-8"),
            "--model-revision",
            model_revision,
            "--replace-record-id",
            existing_record_id,
        ])
        .output()
        .expect("run acceptance requalification import binary")
}

fn vllm_run(
    resilience: Option<InferenceEngineAcceptanceResilience>,
) -> InferenceEngineAcceptanceRun {
    let registry = ExternalInferenceEngineRegistry::standard().expect("standard registry");
    let adapter = registry
        .adapter(InferenceEngineKind::Vllm)
        .expect("vLLM adapter");
    let manifest = adapter.manifest();
    let unit = manifest
        .support_units
        .iter()
        .find(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        .expect("connected vLLM support unit");
    let status = InferenceEngineSupportStatus::VerifiedExternal;
    let evidence = InferenceEngineSupportEvidenceSummary::for_status(status)
        .verified
        .into_iter()
        .map(|kind| InferenceEngineAcceptanceEvidence {
            kind,
            source: "crates/hal100-infra/tests/acceptance_import.rs".to_owned(),
            assertion: "reviewed acceptance import integration fixture".to_owned(),
        })
        .collect();

    InferenceEngineAcceptanceRun {
        schema_version: hal100_infra::INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION,
        run_id: "run-acceptance-import-vllm".to_owned(),
        adapter_id: manifest.adapter_id,
        instance_id: "acceptance:import-test".to_owned(),
        origin_fingerprint: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .to_owned(),
        config_revision: 1,
        protocol_capability_hash: adapter
            .protocol_capability_hash()
            .expect("vLLM protocol contract"),
        platform: unit.platform,
        architecture: unit.architecture,
        accelerator: unit.accelerator,
        deployment: unit.deployment,
        outcome: InferenceEngineAcceptanceRunOutcome::Passed,
        observed_at_ms: 1,
        engine_version: Some("0.10.2".to_owned()),
        deployment_fingerprint: None,
        model_revision: Some("model-id-sha256:0123456789abcdef".to_owned()),
        host_summary: Some("linux/x86_64/cuda".to_owned()),
        host_attestation: common::fixture_host_attestation(
            unit.platform,
            unit.architecture,
            unit.accelerator,
        ),
        model_evidence:
            hal100_infra::InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(
                &hal100_protocol::RuntimeProfileEvidence {
                    kind: hal100_protocol::RuntimeProfileEvidenceKind::CatalogIdentity,
                    algorithm: "vllm-model-id".to_owned(),
                    value: "acceptance-import-model".to_owned(),
                },
            )
            .expect("bounded model evidence"),
        stability: Some(InferenceEngineAcceptanceStability {
            workload_revision: hal100_infra::OPENAI_STABILITY_WORKLOAD_REVISION.to_owned(),
            attempts: 3,
            concurrency: 1,
            p95_latency_ms: 100,
            max_latency_ms: 100,
            total_prompt_tokens: 6,
            total_completion_tokens: 3,
            wall_time_ms: 300,
        }),
        resilience,
        evidence,
    }
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    let bytes = serde_json::to_vec_pretty(value).expect("serialize JSON fixture");
    fs::write(path, bytes).expect("write JSON fixture");
}

#[test]
fn acceptance_import_requires_complete_resilience_and_is_atomic() {
    let temp = TempDir::new();
    let run_path = temp.path().join("run.json");
    let ledger_path = temp.path().join("ledger.json");
    let rejected_output = temp.path().join("rejected.json");
    let accepted_output = temp.path().join("accepted.json");
    let replacement_output = temp.path().join("replacement.json");
    let model_revision = "vllm-0.10.2-qwen3-validated";
    let ledger = InferenceEngineAcceptanceLedger::standard().expect("standard ledger");
    let initial_record_count = ledger.records.len();
    write_json(&ledger_path, &ledger);
    let original_ledger = fs::read(&ledger_path).expect("read original ledger");

    // A complete protocol/stability run without the structured control-plane checks is still a
    // partial artifact. The CLI must reject it before creating a candidate ledger and must leave
    // the source ledger byte-for-byte unchanged.
    write_json(&run_path, &vllm_run(None));
    let rejected = run_import(&run_path, &ledger_path, &rejected_output, model_revision);
    assert!(
        !rejected.status.success(),
        "partial run unexpectedly imported"
    );
    assert!(
        !rejected_output.exists(),
        "rejected import created output ledger"
    );
    assert_eq!(
        fs::read(&ledger_path).expect("read ledger after rejected import"),
        original_ledger
    );

    // Supplying all three resilience checks allows the binary to append a formal record, project
    // the exact vLLM support cell through the standard registry, and write a new candidate file.
    write_json(
        &run_path,
        &vllm_run(Some(InferenceEngineAcceptanceResilience::complete())),
    );
    let accepted = run_import(&run_path, &ledger_path, &accepted_output, model_revision);
    assert!(
        accepted.status.success(),
        "complete run failed to import: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let candidate = InferenceEngineAcceptanceLedger::parse(
        &fs::read(&accepted_output).expect("read accepted candidate ledger"),
    )
    .expect("parse accepted candidate ledger");
    assert_eq!(candidate.records.len(), initial_record_count + 1);
    let record = candidate
        .records
        .iter()
        .find(|record| record.id == "run-acceptance-import-vllm")
        .expect("new vLLM record");
    assert_eq!(
        record.status,
        InferenceEngineSupportStatus::VerifiedExternal
    );
    assert_eq!(record.model_revision.as_deref(), Some(model_revision));
    assert_eq!(
        record.resilience,
        Some(InferenceEngineAcceptanceResilience::complete())
    );
    assert_eq!(
        fs::read(&ledger_path).expect("read ledger after accepted import"),
        original_ledger,
        "source ledger must remain unchanged until an explicit review replaces it"
    );

    // Requalification names the old record and may replace only that same exact support cell.
    // The record count remains stable and both source files stay byte-for-byte unchanged.
    let mut replacement_run = vllm_run(Some(InferenceEngineAcceptanceResilience::complete()));
    replacement_run.run_id = "run-acceptance-import-vllm-requalified".to_owned();
    replacement_run.observed_at_ms = 2;
    write_json(&run_path, &replacement_run);
    let replaced = run_requalification_import(
        &run_path,
        &accepted_output,
        &replacement_output,
        model_revision,
        "run-acceptance-import-vllm",
    );
    assert!(
        replaced.status.success(),
        "same-cell requalification failed to import: {}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    let replacement = InferenceEngineAcceptanceLedger::parse(
        &fs::read(&replacement_output).expect("read replacement candidate ledger"),
    )
    .expect("parse replacement candidate ledger");
    assert_eq!(replacement.records.len(), initial_record_count + 1);
    assert!(
        replacement
            .records
            .iter()
            .any(|record| record.id == "run-acceptance-import-vllm-requalified")
    );
    assert!(
        replacement
            .records
            .iter()
            .all(|record| record.id != "run-acceptance-import-vllm")
    );
    assert_eq!(
        fs::read(&accepted_output).expect("read prior candidate after replacement import"),
        serde_json::to_vec_pretty(&candidate)
            .map(|mut bytes| {
                bytes.push(b'\n');
                bytes
            })
            .expect("serialize prior candidate"),
        "replacement import must not overwrite its input candidate"
    );
}
