use std::{collections::HashMap, env, io, path::PathBuf, process};

use hal100_infra::{
    ExternalInferenceEngineRegistry, InferenceEngineAcceptanceLedger,
    InferenceEngineManifestRegistry, InferenceEngineSupportCellCoverage,
    InferenceEngineSupportCoverageReport,
    build_support_coverage_report_with_protocol_capability_hashes, llama_cpp_manifest,
    read_managed_file,
};
use hal100_protocol::InferenceEngineSupportStatus;

const MAX_LEDGER_BYTES: u64 = 2 * 1024 * 1024;

struct Arguments {
    ledger_path: Option<PathBuf>,
    json: bool,
    strict: bool,
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
    match run(arguments) {
        Ok(strict_failed) if strict_failed => process::exit(2),
        Ok(_) => {}
        Err(error) => {
            eprintln!("HAL100 engine support report failed: {error}");
            process::exit(1);
        }
    }
}

fn run(arguments: Vec<String>) -> Result<bool, Box<dyn std::error::Error>> {
    let arguments = parse_arguments(arguments)?;
    let ledger = match arguments.ledger_path {
        Some(path) => {
            let bytes = read_managed_file(&path, MAX_LEDGER_BYTES)?;
            InferenceEngineAcceptanceLedger::parse(&bytes)?
        }
        None => InferenceEngineAcceptanceLedger::standard()?,
    };
    let external_engine_registry = ExternalInferenceEngineRegistry::standard()?;
    let expected_protocol_capability_hashes = external_engine_registry
        .adapters()
        .into_iter()
        .filter_map(|adapter| {
            adapter
                .protocol_capability_hash()
                .map(|hash| (adapter.manifest().adapter_id, hash))
        })
        .collect::<HashMap<_, _>>();
    let mut manifests = external_engine_registry.manifest_registry().manifests();
    manifests.push(llama_cpp_manifest());
    let registry = InferenceEngineManifestRegistry::new(manifests)?;
    let report = build_support_coverage_report_with_protocol_capability_hashes(
        &registry,
        &ledger,
        &expected_protocol_capability_hashes,
    )?;
    if arguments.json {
        let mut output = serde_json::to_vec_pretty(&report)?;
        output.push(b'\n');
        print!("{}", String::from_utf8(output)?);
    } else {
        print_text_report(&report);
    }
    Ok(arguments.strict && !report.ready_for_strict_promotion)
}

fn parse_arguments<I>(arguments: I) -> Result<Arguments, io::Error>
where
    I: IntoIterator<Item = String>,
{
    let mut ledger_path = None;
    let mut json = false;
    let mut strict = false;
    let mut args = arguments.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--ledger" => {
                let value = args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--ledger requires a value")
                })?;
                ledger_path = Some(PathBuf::from(value));
            }
            "--json" => json = true,
            "--strict" => strict = true,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown argument {other}\n{}", usage()),
                ));
            }
        }
    }
    Ok(Arguments {
        ledger_path,
        json,
        strict,
    })
}

fn print_text_report(report: &InferenceEngineSupportCoverageReport) {
    println!(
        "schema={} adapters={} cells={}/{} formal pending={} ledgerRecords={} performanceProfiles={} formalExternalMissingPerformance={} strictReady={}",
        report.schema_version,
        report.adapters.len(),
        report.formal_support_cells,
        report.total_support_cells,
        report.pending_support_cells,
        report.ledger_records,
        report.reviewed_performance_profiles,
        report.formal_external_cells_missing_performance_profile,
        report.ready_for_strict_promotion,
    );
    for adapter in &report.adapters {
        println!(
            "{} [{}] formal={}/{} pending={} ledgerBacked={} strictReady={}",
            adapter.display_name,
            adapter.adapter_id.engine.storage_key(),
            adapter.formal_support_cells,
            adapter.total_support_cells,
            adapter.pending_support_cells,
            adapter.ledger_backed_cells,
            adapter.ready_for_strict_promotion,
        );
        for cell in &adapter.cells {
            println!(
                "  {} / {} / {} / {}: manifest={} effective={} ledger={} performanceProfile={} promotionReady={}",
                platform_key(cell),
                cell.architecture.storage_key(),
                cell.accelerator.storage_key(),
                cell.deployment.storage_key(),
                status_key(cell.manifest_status),
                status_key(cell.effective_status),
                cell.ledger_record_present,
                cell.reviewed_performance_profile.is_some(),
                cell.promotion_ready,
            );
        }
    }
}

fn platform_key(cell: &InferenceEngineSupportCellCoverage) -> &'static str {
    match cell.platform {
        hal100_protocol::InferencePlatform::MacOs => "macos",
        hal100_protocol::InferencePlatform::Windows => "windows",
        hal100_protocol::InferencePlatform::Linux => "linux",
    }
}

const fn status_key(status: InferenceEngineSupportStatus) -> &'static str {
    match status {
        InferenceEngineSupportStatus::Reserved => "reserved",
        InferenceEngineSupportStatus::Connected => "connected",
        InferenceEngineSupportStatus::VerifiedExternal => "verifiedExternal",
        InferenceEngineSupportStatus::Managed => "managed",
    }
}

const fn usage() -> &'static str {
    "usage: hal100-engine-support-report [--ledger LEDGER.json] [--json] [--strict]"
}
