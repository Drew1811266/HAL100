#!/usr/bin/env bash

set -u
set -o pipefail
umask 077

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
run_stamp=$(date -u '+%Y%m%dT%H%M%SZ')
output_dir="$workspace_root/output/stability/matrix-$run_stamp"
include_opencode=1

usage() {
  printf '%s\n' \
    "Usage: run_iteration9_matrix.sh [options]" \
    "" \
    "Options:" \
    "  --output-dir PATH    Report directory" \
    "  --skip-opencode      Skip the network-dependent official OpenCode CLI matrix" \
    "  --help               Show this help"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-dir)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      output_dir=$2
      shift 2
      ;;
    --skip-opencode)
      include_opencode=0
      shift
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

mkdir -p "$output_dir"
chmod 700 "$output_dir"
summary_path="$output_dir/summary.md"
started_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
passed=0
failed=0

{
  printf '# HAL100 迭代 9 快速验收矩阵\n\n'
  printf -- '- 开始：%s\n' "$started_utc"
  printf -- '- 工作区：%s\n' "$workspace_root"
  printf -- '- OpenCode 官方 CLI：%s\n\n' "$include_opencode"
  printf '| 阶段 | 结果 | 日志 |\n'
  printf '| --- | --- | --- |\n'
} > "$summary_path"

run_stage() {
  stage_id=$1
  stage_label=$2
  shift 2
  log_path="$output_dir/$stage_id.log"
  printf 'matrix stage=%s started\n' "$stage_id"
  if "$@" > >(tee "$log_path") 2>&1; then
    result=通过
    passed=$((passed + 1))
  else
    result=失败
    failed=$((failed + 1))
  fi
  printf '| %s | %s | `%s` |\n' "$stage_label" "$result" "$(basename "$log_path")" >> "$summary_path"
  printf 'matrix stage=%s result=%s\n' "$stage_id" "$result"
}

cd "$workspace_root"

run_stage full_check "全量静态检查与默认测试" pnpm check
run_stage production_build "连续开发版生产构建" pnpm build
run_stage sqlite_busy "SQLite 占锁超时与恢复" \
  cargo test -p hal100-infra --lib \
  database::tests::database_busy_failure_is_bounded_and_writes_resume_after_lock_release \
  -- --ignored --exact --nocapture
run_stage million_usage "100 万 Usage 查询与清理" \
  cargo test -p hal100-infra --lib \
  database::tests::million_usage_rows_remain_queryable_and_cleanable_within_the_scale_budget \
  -- --ignored --exact --nocapture
run_stage model_library_scale "1 万模型快照刷新与查询" \
  cargo test -p hal100-infra --lib \
  database::tests::ten_thousand_model_snapshots_refresh_and_list_within_the_scale_budget \
  -- --ignored --exact --nocapture
run_stage sidecar_roundtrip "真实 Sidecar 工具往返" \
  cargo test -p hal100-core --test pi_tool_broker_e2e \
  rust_broker_completes_a_real_pi_tool_round_trip -- --ignored --exact --nocapture
run_stage sidecar_lifecycle "Sidecar 连续 25 次启停" \
  cargo test -p hal100-core --test pi_tool_broker_e2e \
  real_sidecar_starts_pings_and_exits_cleanly_twenty_five_times \
  -- --ignored --exact --nocapture
run_stage sidecar_oversized_rpc "Sidecar 超大 RPC 帧故障关闭" \
  cargo test -p hal100-core --test pi_tool_broker_e2e \
  oversized_rpc_frame_terminates_the_real_sidecar_without_hanging \
  -- --ignored --exact --nocapture
run_stage gateway_latency "Gateway 本机 p95 延迟" \
  cargo test -p hal100-infra --test gateway_e2e \
  gateway_p95_overhead_stays_below_five_milliseconds \
  -- --ignored --exact --nocapture

if [ "$include_opencode" -eq 1 ]; then
  for version in 1.18.11 1.17.9; do
    stage_version=${version//./_}
    run_stage "opencode_$stage_version" "OpenCode $version 官方 CLI" \
      env HAL100_OPENCODE_TEST_PACKAGE="opencode-ai@$version" \
      cargo test -p hal100-infra --test opencode_cli_e2e \
      official_opencode_cli_uses_managed_provider_and_records_usage \
      -- --ignored --exact --nocapture
  done
fi

completed_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
{
  printf '\n- 完成：%s\n' "$completed_utc"
  printf -- '- 通过阶段：%s\n' "$passed"
  printf -- '- 失败阶段：%s\n' "$failed"
} >> "$summary_path"

printf 'matrix complete passed=%s failed=%s report=%s\n' "$passed" "$failed" "$summary_path"
[ "$failed" -eq 0 ]
