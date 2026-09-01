[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('ollama', 'mlx-lm', 'mlc-llm', 'openvino', 'vllm', 'sglang', 'lmdeploy', 'tensorrt-llm')]
    [string] $Engine
)

$ErrorActionPreference = 'Stop'

if ($env:HAL100_RUN_REAL_ACCEPTANCE -ne '1') {
    throw 'Refusing live inference requests: set HAL100_RUN_REAL_ACCEPTANCE=1 explicitly.'
}

$entries = @{
    'ollama' = @{ Test = 'ollama_live_acceptance'; Function = 'fixed_local_ollama_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_OLLAMA_ACCEPTANCE' }
    'mlx-lm' = @{ Test = 'mlx_lm_live_acceptance'; Function = 'fixed_local_mlx_lm_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_MLX_LM_ACCEPTANCE' }
    'mlc-llm' = @{ Test = 'mlc_llm_live_acceptance'; Function = 'fixed_local_mlc_llm_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_MLC_LLM_ACCEPTANCE' }
    'openvino' = @{ Test = 'openvino_live_acceptance'; Function = 'fixed_local_openvino_model_server_passes_the_agent_protocol_vertical'; Ack = 'HAL100_OPENVINO_ACCEPTANCE' }
    'vllm' = @{ Test = 'vllm_live_acceptance'; Function = 'fixed_local_vllm_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_VLLM_ACCEPTANCE' }
    'sglang' = @{ Test = 'sglang_live_acceptance'; Function = 'fixed_local_sglang_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_SGLANG_ACCEPTANCE' }
    'lmdeploy' = @{ Test = 'lmdeploy_live_acceptance'; Function = 'fixed_local_lmdeploy_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_LMDEPLOY_ACCEPTANCE' }
    'tensorrt-llm' = @{ Test = 'tensorrt_llm_live_acceptance'; Function = 'fixed_local_tensorrt_llm_service_passes_the_agent_protocol_vertical'; Ack = 'HAL100_TENSORRT_LLM_ACCEPTANCE' }
}

$entry = $entries[$Engine]
$acknowledgement = [Environment]::GetEnvironmentVariable($entry.Ack, 'Process')
if ($acknowledgement -and $acknowledgement -ne '1') {
    throw "$($entry.Ack) must be empty or 1; refusing an ambiguous acknowledgement."
}
[Environment]::SetEnvironmentVariable($entry.Ack, '1', 'Process')

# Validate exact engine environment wiring without printing values or issuing a request. The Rust
# test remains authoritative for native hardware and live service qualification.
node scripts/validate-engine-acceptance-environment.mjs --engine $Engine
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$outputPath = $env:HAL100_ACCEPTANCE_EVIDENCE_OUT
if ([string]::IsNullOrWhiteSpace($outputPath)) {
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')
    $outputPath = Join-Path 'output/inference-acceptance' "$Engine-$stamp.json"
}
if (Test-Path -LiteralPath $outputPath) {
    throw "Refusing to overwrite existing acceptance artifact: $outputPath"
}

$outputDirectory = Split-Path -Parent $outputPath
if ($outputDirectory) {
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
}

$env:HAL100_ACCEPTANCE_EVIDENCE_WRITE = '1'
$env:HAL100_ACCEPTANCE_EVIDENCE_OUT = $outputPath

# The Rust test validates the loopback target, native host snapshot, engine identity, protocol
# qualification, lifecycle, bounded stability and shared control-plane resilience. Keep the test
# name exact and fixed. This wrapper never starts, installs, downloads, stops or reconfigures an
# engine; the target service and model must already be prepared by the operator.
cargo test -p hal100-infra --test $entry.Test -- --ignored --exact $entry.Function
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Acceptance artifact written (create-new): $outputPath"
Write-Host 'Review the artifact and import it with hal100-engine-acceptance-import; no support status was changed.'
