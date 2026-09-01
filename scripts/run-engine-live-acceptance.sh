#!/usr/bin/env bash

set -euo pipefail

engine="${1:-}"
if [[ -z "$engine" || "$engine" == "--help" || "$engine" == "-h" ]]; then
  cat <<'USAGE'
Usage: HAL100_RUN_REAL_ACCEPTANCE=1 scripts/run-engine-live-acceptance.sh ENGINE

ENGINE must be one of:
  ollama | mlx-lm | mlc-llm | openvino | vllm | sglang | lmdeploy | tensorrt-llm

The target service, model, version/fingerprint and accelerator must already be prepared by the
operator. This script never starts, installs, downloads, stops or reconfigures an engine.
USAGE
  [[ -n "$engine" ]] && exit 0
  exit 2
fi

if [[ "${HAL100_RUN_REAL_ACCEPTANCE:-}" != "1" ]]; then
  printf '%s\n' 'Refusing live inference requests: set HAL100_RUN_REAL_ACCEPTANCE=1 explicitly.' >&2
  exit 2
fi

case "$engine" in
  ollama)
    test_name="ollama_live_acceptance"
    function_name="fixed_local_ollama_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_OLLAMA_ACCEPTANCE"
    ;;
  mlx-lm)
    test_name="mlx_lm_live_acceptance"
    function_name="fixed_local_mlx_lm_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_MLX_LM_ACCEPTANCE"
    ;;
  mlc-llm)
    test_name="mlc_llm_live_acceptance"
    function_name="fixed_local_mlc_llm_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_MLC_LLM_ACCEPTANCE"
    ;;
  openvino)
    test_name="openvino_live_acceptance"
    function_name="fixed_local_openvino_model_server_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_OPENVINO_ACCEPTANCE"
    ;;
  vllm)
    test_name="vllm_live_acceptance"
    function_name="fixed_local_vllm_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_VLLM_ACCEPTANCE"
    ;;
  sglang)
    test_name="sglang_live_acceptance"
    function_name="fixed_local_sglang_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_SGLANG_ACCEPTANCE"
    ;;
  lmdeploy)
    test_name="lmdeploy_live_acceptance"
    function_name="fixed_local_lmdeploy_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_LMDEPLOY_ACCEPTANCE"
    ;;
  tensorrt-llm)
    test_name="tensorrt_llm_live_acceptance"
    function_name="fixed_local_tensorrt_llm_service_passes_the_agent_protocol_vertical"
    acceptance_var="HAL100_TENSORRT_LLM_ACCEPTANCE"
    ;;
  *)
    printf 'Unsupported engine: %s\n' "$engine" >&2
    exit 2
    ;;
esac

if [[ -n "${!acceptance_var:-}" && "${!acceptance_var}" != "1" ]]; then
  printf '%s must be empty or 1; refusing an ambiguous acknowledgement.\n' "$acceptance_var" >&2
  exit 2
fi
export "$acceptance_var=1"

# Validate exact engine environment wiring without printing values or issuing a request. The Rust
# test remains authoritative for native hardware and live service qualification.
node scripts/validate-engine-acceptance-environment.mjs --engine "$engine"

output_path="${HAL100_ACCEPTANCE_EVIDENCE_OUT:-output/inference-acceptance/${engine}-$(date -u +%Y%m%dT%H%M%SZ).json}"
if [[ -e "$output_path" ]]; then
  printf 'Refusing to overwrite existing acceptance artifact: %s\n' "$output_path" >&2
  exit 2
fi
output_dir="${output_path%/*}"
if [[ "$output_dir" != "$output_path" ]]; then
  mkdir -p "$output_dir"
fi

export HAL100_ACCEPTANCE_EVIDENCE_WRITE=1
export HAL100_ACCEPTANCE_EVIDENCE_OUT="$output_path"

# The Rust test validates the loopback target, native host snapshot, engine identity, protocol
# qualification, lifecycle, bounded stability and shared control-plane resilience. Keep the test
# name exact and fixed.
cargo test -p hal100-infra --test "$test_name" -- --ignored --exact "$function_name"

printf 'Acceptance artifact written (create-new): %s\n' "$output_path"
printf '%s\n' 'Review the artifact and import it with hal100-engine-acceptance-import; no support status was changed.'
