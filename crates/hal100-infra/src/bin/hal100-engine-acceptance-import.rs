use std::{env, io, path::PathBuf, process};

use hal100_infra::{
    ExternalInferenceEngineRegistry, InferenceEngineAcceptanceEvidenceError,
    InferenceEngineAcceptanceLedger, InferenceEngineAcceptanceRun,
    InferenceEngineAcceptanceRunOutcome, read_managed_file, write_new_managed_file,
};
use hal100_protocol::InferenceEngineSupportStatus;

const MAX_RUN_BYTES: u64 = 512 * 1024;
const MAX_LEDGER_BYTES: u64 = 2 * 1024 * 1024;

struct Arguments {
    run_path: PathBuf,
    ledger_path: PathBuf,
    output_path: PathBuf,
    model_revision: String,
    replace_record_id: Option<String>,
}

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "--help" | "-h"))
    {
        println!("{}", usage());
        return;
    }
    if let Err(error) = run(arguments) {
        eprintln!("HAL100 acceptance import failed: {error}");
        process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(arguments)?;
    let run_bytes = read_managed_file(&arguments.run_path, MAX_RUN_BYTES)?;
    let ledger_bytes = read_managed_file(&arguments.ledger_path, MAX_LEDGER_BYTES)?;
    let run = serde_json::from_slice::<InferenceEngineAcceptanceRun>(&run_bytes)
        .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidJson)?;
    if !matches!(run.outcome, InferenceEngineAcceptanceRunOutcome::Passed) {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord.into());
    }
    let mut ledger = InferenceEngineAcceptanceLedger::parse(&ledger_bytes)?;
    let record = run.into_formal_record_with_model_revision(
        InferenceEngineSupportStatus::VerifiedExternal,
        &arguments.model_revision,
    )?;
    if let Some(existing_record_id) = arguments.replace_record_id.as_deref() {
        ledger.replace_reviewed_record(existing_record_id, record.clone())?;
    } else {
        ledger.append_reviewed_record(record.clone())?;
    }

    // Validate against every standard adapter before writing a candidate. This catches a stale,
    // misspelled or cross-platform support cell while the original ledger remains untouched.
    ExternalInferenceEngineRegistry::standard_with_reviewed_acceptance_ledger(&ledger)
        .map_err(|_| InferenceEngineAcceptanceEvidenceError::RecordMismatch)?;

    let mut output = serde_json::to_vec_pretty(&ledger)
        .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidJson)?;
    output.push(b'\n');
    write_new_managed_file(&arguments.output_path, &output, 0o644)?;
    println!(
        "imported {} {} {} {} -> {}",
        record.adapter_id.engine.storage_key(),
        record.adapter_id.variant,
        platform_key(record.platform),
        cell_key(record.architecture, record.accelerator),
        arguments.output_path.display()
    );
    Ok(())
}

fn parse_arguments<I>(arguments: I) -> Result<Arguments, io::Error>
where
    I: IntoIterator<Item = String>,
{
    let mut run_path = None;
    let mut ledger_path = None;
    let mut output_path = None;
    let mut model_revision = None;
    let mut replace_record_id = None;
    let mut args = arguments.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--run" => run_path = Some(PathBuf::from(required_value(&mut args, "--run")?)),
            "--ledger" => ledger_path = Some(PathBuf::from(required_value(&mut args, "--ledger")?)),
            "--output" => output_path = Some(PathBuf::from(required_value(&mut args, "--output")?)),
            "--model-revision" => {
                model_revision = Some(required_value(&mut args, "--model-revision")?)
            }
            "--replace-record-id" => {
                replace_record_id = Some(required_value(&mut args, "--replace-record-id")?)
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown argument {other}\n{}", usage()),
                ));
            }
        }
    }
    Ok(Arguments {
        run_path: required_path(run_path, "--run")?,
        ledger_path: required_path(ledger_path, "--ledger")?,
        output_path: required_path(output_path, "--output")?,
        model_revision: model_revision.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("--model-revision requires a value\n{}", usage()),
            )
        })?,
        replace_record_id,
    })
}

fn required_value<I>(arguments: &mut I, name: &str) -> Result<String, io::Error>
where
    I: Iterator<Item = String>,
{
    arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} requires a value"),
        )
    })
}

fn required_path(value: Option<PathBuf>, name: &str) -> Result<PathBuf, io::Error> {
    value.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} requires a value\n{}", usage()),
        )
    })
}

const fn usage() -> &'static str {
    "usage: hal100-engine-acceptance-import --run RUN.json --ledger LEDGER.json --output NEW_LEDGER.json --model-revision REVISION [--replace-record-id EXISTING_ID]"
}

const fn platform_key(platform: hal100_protocol::InferencePlatform) -> &'static str {
    match platform {
        hal100_protocol::InferencePlatform::MacOs => "macos",
        hal100_protocol::InferencePlatform::Windows => "windows",
        hal100_protocol::InferencePlatform::Linux => "linux",
    }
}

fn cell_key(
    architecture: hal100_protocol::InferenceArchitecture,
    accelerator: hal100_protocol::InferenceAccelerator,
) -> String {
    // This function is only used for a compact operator-facing summary. Keep the values bounded
    // and derived from the typed enums rather than echoing any run-artifact text.
    format!(
        "{}/{}",
        match architecture {
            hal100_protocol::InferenceArchitecture::Aarch64 => "aarch64",
            hal100_protocol::InferenceArchitecture::X86_64 => "x86_64",
        },
        match accelerator {
            hal100_protocol::InferenceAccelerator::Cpu => "cpu",
            hal100_protocol::InferenceAccelerator::Metal => "metal",
            hal100_protocol::InferenceAccelerator::Cuda => "cuda",
            hal100_protocol::InferenceAccelerator::Rocm => "rocm",
            hal100_protocol::InferenceAccelerator::Vulkan => "vulkan",
            hal100_protocol::InferenceAccelerator::Sycl => "sycl",
            hal100_protocol::InferenceAccelerator::IntelGpu => "intel_gpu",
            hal100_protocol::InferenceAccelerator::IntelNpu => "intel_npu",
        }
    )
}
