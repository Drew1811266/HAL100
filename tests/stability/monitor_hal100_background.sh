#!/usr/bin/env bash

set -u
set -o pipefail
umask 077

usage() {
  printf '%s\n' \
    "Usage: monitor_hal100_background.sh [options]" \
    "" \
    "Options:" \
    "  --pid PID                 Existing HAL100 desktop PID (auto-detected by default)" \
    "  --duration-seconds N      Observation window (default: 3600)" \
    "  --interval-seconds N      Sampling interval (default: 30)" \
    "  --output-dir PATH         Report directory" \
    "  --gateway-url URL         Health endpoint (default: http://127.0.0.1:10100/healthz)" \
    "  --app-data-dir PATH       HAL100 app data directory" \
    "  --app-log-dir PATH        HAL100 app log directory" \
    "  --help                    Show this help"
}

is_positive_integer() {
  case "$1" in
    ''|*[!0-9]*|0) return 1 ;;
    *) return 0 ;;
  esac
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
duration_seconds=3600
interval_seconds=30
hal100_pid=""
gateway_url="http://127.0.0.1:10100/healthz"
app_data_dir="/Users/${USER}/Library/Application Support/com.hal100.desktop"
app_log_dir="/Users/${USER}/Library/Logs/com.hal100.desktop"
output_dir=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --pid)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      hal100_pid=$2
      shift 2
      ;;
    --duration-seconds)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      duration_seconds=$2
      shift 2
      ;;
    --interval-seconds)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      interval_seconds=$2
      shift 2
      ;;
    --output-dir)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      output_dir=$2
      shift 2
      ;;
    --gateway-url)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      gateway_url=$2
      shift 2
      ;;
    --app-data-dir)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      app_data_dir=$2
      shift 2
      ;;
    --app-log-dir)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      app_log_dir=$2
      shift 2
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

is_positive_integer "$duration_seconds" || {
  printf 'duration must be a positive integer\n' >&2
  exit 2
}
is_positive_integer "$interval_seconds" || {
  printf 'interval must be a positive integer\n' >&2
  exit 2
}

if [ -z "$hal100_pid" ]; then
  hal100_pid=$(pgrep -f "$workspace_root/target/(debug|release)/hal100-desktop" | head -n 1 || true)
fi
is_positive_integer "$hal100_pid" || {
  printf 'HAL100 desktop process was not found; pass --pid explicitly\n' >&2
  exit 2
}

process_command=$(ps -p "$hal100_pid" -o command= 2>/dev/null || true)
case "$process_command" in
  "$workspace_root"/target/debug/hal100-desktop|"$workspace_root"/target/release/hal100-desktop) ;;
  *)
    printf 'PID %s is not the HAL100 binary in this workspace: %s\n' "$hal100_pid" "$process_command" >&2
    exit 2
    ;;
esac

if [ -z "$output_dir" ]; then
  run_stamp=$(date -u '+%Y%m%dT%H%M%SZ')
  output_dir="$workspace_root/output/stability/$run_stamp"
fi
mkdir -p "$output_dir"
chmod 700 "$output_dir"

metrics_path="$output_dir/metrics.csv"
summary_env_path="$output_dir/summary.env"
summary_json_path="$output_dir/summary.json"
summary_markdown_path="$output_dir/summary.md"
metadata_path="$output_dir/metadata.txt"
session_dir="$app_data_dir/agent/sessions"
database_path="$app_data_dir/hal100.sqlite"

started_epoch=$(date '+%s')
started_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
interrupted=0
trap 'interrupted=1' INT TERM

usage_count_before=0
audit_count_before=0
if [ -f "$database_path" ]; then
  usage_count_before=$(sqlite3 "$database_path" 'SELECT COUNT(*) FROM usage_requests;' 2>/dev/null || printf '0')
  audit_count_before=$(sqlite3 "$database_path" 'SELECT COUNT(*) FROM audit_events;' 2>/dev/null || printf '0')
fi

{
  printf 'started_utc=%s\n' "$started_utc"
  printf 'workspace=%s\n' "$workspace_root"
  printf 'pid=%s\n' "$hal100_pid"
  printf 'command=%s\n' "$process_command"
  printf 'duration_seconds=%s\n' "$duration_seconds"
  printf 'interval_seconds=%s\n' "$interval_seconds"
  printf 'gateway_url=%s\n' "$gateway_url"
  printf 'architecture=%s\n' "$(uname -m)"
  printf 'os_version=%s\n' "$(sw_vers -productVersion 2>/dev/null || uname -r)"
  printf 'usage_count_before=%s\n' "$usage_count_before"
  printf 'audit_count_before=%s\n' "$audit_count_before"
} > "$metadata_path"

printf '%s\n' 'sample,utc,elapsed_seconds,alive,cpu_percent,rss_kib,physical_mib,peak_physical_mib,threads,open_files,tcp_connections,direct_children,agent_children,session_directories,gateway_healthy,log_kib' > "$metrics_path"

vmmap_metrics() {
  vmmap -summary "$hal100_pid" 2>/dev/null | awk '
    function to_mib(raw, unit, number) {
      unit = substr(raw, length(raw), 1)
      number = substr(raw, 1, length(raw) - 1) + 0
      if (unit == "K") return number / 1024
      if (unit == "G") return number * 1024
      if (unit == "M") return number
      return raw / 1048576
    }
    /Physical footprint:/ { physical = to_mib($NF) }
    /Physical footprint \(peak\):/ { peak = to_mib($NF) }
    END { printf "%.3f %.3f", physical + 0, peak + 0 }
  '
}

sample_index=0
while :; do
  now_epoch=$(date '+%s')
  elapsed_seconds=$((now_epoch - started_epoch))
  sample_index=$((sample_index + 1))
  now_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

  alive=0
  cpu_percent=0
  rss_kib=0
  physical_mib=0
  peak_physical_mib=0
  threads=0
  open_files=0
  tcp_connections=0
  direct_children=0
  agent_children=0
  session_directories=0
  gateway_healthy=0
  log_kib=0

  if kill -0 "$hal100_pid" 2>/dev/null; then
    alive=1
    cpu_percent=$(ps -p "$hal100_pid" -o %cpu= 2>/dev/null | tr -d ' ' || printf '0')
    rss_kib=$(ps -p "$hal100_pid" -o rss= 2>/dev/null | tr -d ' ' || printf '0')
    vmmap_values=$(vmmap_metrics || printf '0 0')
    physical_mib=${vmmap_values%% *}
    peak_physical_mib=${vmmap_values##* }
    threads=$(ps -M -p "$hal100_pid" 2>/dev/null | awk 'NR > 1 { count += 1 } END { print count + 0 }')
    open_files=$(lsof -n -p "$hal100_pid" 2>/dev/null | awk 'NR > 1 { count += 1 } END { print count + 0 }')
    tcp_connections=$(lsof -nP -a -p "$hal100_pid" -iTCP 2>/dev/null | awk 'NR > 1 { count += 1 } END { print count + 0 }')
    direct_children=$(ps -axo ppid= 2>/dev/null | awk -v pid="$hal100_pid" '$1 == pid { count += 1 } END { print count + 0 }')
  fi

  agent_children=$(ps -axo ppid=,command= 2>/dev/null | awk -v pid="$hal100_pid" '
    $1 == pid && ($0 ~ /agent-kernel\/dist\/index/ || $0 ~ /llama-server/ || $0 ~ /hal100-agent/) { count += 1 }
    END { print count + 0 }
  ')
  if [ -d "$session_dir" ]; then
    session_directories=$(find "$session_dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | awk 'END { print NR + 0 }')
  fi
  if curl --fail --silent --show-error --max-time 2 "$gateway_url" >/dev/null 2>&1; then
    gateway_healthy=1
  fi
  if [ -d "$app_log_dir" ]; then
    log_kib=$(du -sk "$app_log_dir" 2>/dev/null | awk '{ print $1 + 0 }')
  fi

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$sample_index" "$now_utc" "$elapsed_seconds" "$alive" "$cpu_percent" "$rss_kib" \
    "$physical_mib" "$peak_physical_mib" "$threads" "$open_files" "$tcp_connections" \
    "$direct_children" "$agent_children" "$session_directories" "$gateway_healthy" "$log_kib" \
    >> "$metrics_path"
  printf 'stability sample=%s elapsed=%ss cpu=%s%% physical=%sMiB fds=%s tcp=%s gateway=%s\n' \
    "$sample_index" "$elapsed_seconds" "$cpu_percent" "$physical_mib" "$open_files" \
    "$tcp_connections" "$gateway_healthy"

  if [ "$interrupted" -eq 1 ] || [ "$elapsed_seconds" -ge "$duration_seconds" ]; then
    break
  fi
  remaining_seconds=$((duration_seconds - elapsed_seconds))
  sleep_seconds=$interval_seconds
  if [ "$remaining_seconds" -lt "$sleep_seconds" ]; then
    sleep_seconds=$remaining_seconds
  fi
  sleep "$sleep_seconds"
done

usage_count_after=0
audit_count_after=0
unsafe_audit_rows=0
if [ -f "$database_path" ]; then
  usage_count_after=$(sqlite3 "$database_path" 'SELECT COUNT(*) FROM usage_requests;' 2>/dev/null || printf '0')
  audit_count_after=$(sqlite3 "$database_path" 'SELECT COUNT(*) FROM audit_events;' 2>/dev/null || printf '0')
  unsafe_audit_rows=$(
    sqlite3 "$database_path" 2>/dev/null <<'SQL' || printf '0'
SELECT COUNT(*)
FROM audit_events AS event
WHERE json_valid(event.summary_json) = 0
   OR EXISTS (
     SELECT 1
     FROM json_tree(event.summary_json) AS field
     WHERE (
       field.key IS NOT NULL
       AND field.type IN ('text', 'object', 'array')
       AND (
         lower(CAST(field.key AS TEXT)) LIKE '%prompt%'
         OR lower(CAST(field.key AS TEXT)) LIKE '%answer%'
         OR lower(CAST(field.key AS TEXT)) LIKE '%apikey%'
         OR lower(CAST(field.key AS TEXT)) LIKE '%authorization%'
       )
     )
     OR (
       field.type = 'text'
       AND (
         lower(CAST(field.value AS TEXT)) LIKE '%hal100_agent_session_%'
         OR lower(CAST(field.value AS TEXT)) LIKE '%authorization%bearer%'
         OR lower(CAST(field.value AS TEXT)) LIKE '%x-api-key%'
       )
     )
   );
SQL
  )
fi

secret_match_files=0
if [ -d "$app_log_dir" ]; then
  secret_match_files=$(rg -l -i 'hal100_agent_session_|authorization[" :]+bearer|x-api-key[" :]+[A-Za-z0-9]|"apiKey"' "$app_log_dir" --glob '*.log' --glob '*.jsonl' 2>/dev/null | awk 'END { print NR + 0 }')
fi

awk -F, '
  NR == 2 {
    first_physical = $7
    first_fds = $10
    first_tcp = $11
    first_threads = $9
    first_log = $16
  }
  NR > 1 {
    samples += 1
    cpu_sum += $5
    if ($5 > max_cpu) max_cpu = $5
    if ($7 > max_physical) max_physical = $7
    if ($8 > max_peak) max_peak = $8
    if ($10 > max_fds) max_fds = $10
    if ($11 > max_tcp) max_tcp = $11
    if ($12 > max_children) max_children = $12
    if ($13 > max_agent_children) max_agent_children = $13
    if ($14 > max_sessions) max_sessions = $14
    if ($1 > 1 && $4 == 0) alive_failures += 1
    if ($15 == 0) gateway_failures += 1
    last_elapsed = $3
    last_physical = $7
    last_fds = $10
    last_tcp = $11
    last_threads = $9
    last_log = $16
  }
  END {
    if (samples == 0) samples = 1
    printf "sample_count=%d\n", samples
    printf "observed_seconds=%d\n", last_elapsed
    printf "average_cpu_percent=%.4f\n", cpu_sum / samples
    printf "max_cpu_percent=%.4f\n", max_cpu
    printf "first_physical_mib=%.3f\n", first_physical
    printf "last_physical_mib=%.3f\n", last_physical
    printf "physical_growth_mib=%.3f\n", last_physical - first_physical
    printf "max_physical_mib=%.3f\n", max_physical
    printf "max_peak_physical_mib=%.3f\n", max_peak
    printf "first_open_files=%d\n", first_fds
    printf "last_open_files=%d\n", last_fds
    printf "open_file_growth=%d\n", last_fds - first_fds
    printf "max_open_files=%d\n", max_fds
    printf "tcp_growth=%d\n", last_tcp - first_tcp
    printf "thread_growth=%d\n", last_threads - first_threads
    printf "log_growth_kib=%d\n", last_log - first_log
    printf "max_direct_children=%d\n", max_children
    printf "max_agent_children=%d\n", max_agent_children
    printf "max_session_directories=%d\n", max_sessions
    printf "alive_failures=%d\n", alive_failures
    printf "gateway_failures=%d\n", gateway_failures
  }
' "$metrics_path" > "$summary_env_path"

# shellcheck disable=SC1090
. "$summary_env_path"
completed_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
passed=true
failure_reasons=""

append_failure() {
  if [ -n "$failure_reasons" ]; then
    failure_reasons="$failure_reasons; $1"
  else
    failure_reasons=$1
  fi
  passed=false
}

awk -v value="$average_cpu_percent" 'BEGIN { exit !(value <= 0.3) }' || append_failure "average_cpu_above_0.3_percent"
awk -v value="$first_physical_mib" 'BEGIN { exit !(value > 0) }' || append_failure "physical_memory_metric_unavailable"
awk -v value="$max_physical_mib" 'BEGIN { exit !(value <= 80) }' || append_failure "physical_memory_above_80_mib"
awk -v value="$physical_growth_mib" 'BEGIN { exit !(value <= 8) }' || append_failure "physical_memory_growth_above_8_mib"
[ "$open_file_growth" -le 4 ] || append_failure "open_file_growth_above_4"
[ "$tcp_growth" -le 2 ] || append_failure "tcp_growth_above_2"
[ "$thread_growth" -le 2 ] || append_failure "thread_growth_above_2"
[ "$max_agent_children" -eq 0 ] || append_failure "unexpected_agent_child_process"
[ "$max_session_directories" -eq 0 ] || append_failure "unexpected_agent_session_directory"
[ "$alive_failures" -eq 0 ] || append_failure "desktop_process_exited"
[ "$gateway_failures" -eq 0 ] || append_failure "gateway_health_failed"
[ "$unsafe_audit_rows" -eq 0 ] || append_failure "unsafe_audit_fields_detected"
[ "$secret_match_files" -eq 0 ] || append_failure "possible_secret_in_log"
[ "$interrupted" -eq 0 ] || append_failure "monitor_interrupted"

printf '{\n' > "$summary_json_path"
printf '  "passed": %s,\n' "$passed" >> "$summary_json_path"
printf '  "failureReasons": "%s",\n' "$failure_reasons" >> "$summary_json_path"
printf '  "startedUtc": "%s",\n' "$started_utc" >> "$summary_json_path"
printf '  "completedUtc": "%s",\n' "$completed_utc" >> "$summary_json_path"
printf '  "pid": %s,\n' "$hal100_pid" >> "$summary_json_path"
printf '  "sampleCount": %s,\n' "$sample_count" >> "$summary_json_path"
printf '  "observedSeconds": %s,\n' "$observed_seconds" >> "$summary_json_path"
printf '  "averageCpuPercent": %s,\n' "$average_cpu_percent" >> "$summary_json_path"
printf '  "maxCpuPercent": %s,\n' "$max_cpu_percent" >> "$summary_json_path"
printf '  "firstPhysicalMiB": %s,\n' "$first_physical_mib" >> "$summary_json_path"
printf '  "lastPhysicalMiB": %s,\n' "$last_physical_mib" >> "$summary_json_path"
printf '  "physicalGrowthMiB": %s,\n' "$physical_growth_mib" >> "$summary_json_path"
printf '  "maxPhysicalMiB": %s,\n' "$max_physical_mib" >> "$summary_json_path"
printf '  "openFileGrowth": %s,\n' "$open_file_growth" >> "$summary_json_path"
printf '  "tcpGrowth": %s,\n' "$tcp_growth" >> "$summary_json_path"
printf '  "threadGrowth": %s,\n' "$thread_growth" >> "$summary_json_path"
printf '  "maxAgentChildren": %s,\n' "$max_agent_children" >> "$summary_json_path"
printf '  "maxSessionDirectories": %s,\n' "$max_session_directories" >> "$summary_json_path"
printf '  "gatewayFailures": %s,\n' "$gateway_failures" >> "$summary_json_path"
printf '  "unsafeAuditRows": %s,\n' "$unsafe_audit_rows" >> "$summary_json_path"
printf '  "secretMatchFiles": %s,\n' "$secret_match_files" >> "$summary_json_path"
printf '  "usageCountBefore": %s,\n' "$usage_count_before" >> "$summary_json_path"
printf '  "usageCountAfter": %s,\n' "$usage_count_after" >> "$summary_json_path"
printf '  "auditCountBefore": %s,\n' "$audit_count_before" >> "$summary_json_path"
printf '  "auditCountAfter": %s\n' "$audit_count_after" >> "$summary_json_path"
printf '}\n' >> "$summary_json_path"

{
  printf '# HAL100 后台稳定性观察结果\n\n'
  printf -- '- 结果：%s\n' "$passed"
  printf -- '- 失败原因：%s\n' "${failure_reasons:-无}"
  printf -- '- 时间：%s — %s\n' "$started_utc" "$completed_utc"
  printf -- '- 观察时长：%s 秒，%s 个样本\n' "$observed_seconds" "$sample_count"
  printf -- '- CPU：平均 %s%%，最大 %s%%\n' "$average_cpu_percent" "$max_cpu_percent"
  printf -- '- 物理内存：%s → %s MiB，增长 %s MiB，最大 %s MiB\n' "$first_physical_mib" "$last_physical_mib" "$physical_growth_mib" "$max_physical_mib"
  printf -- '- 资源增长：文件 %s，TCP %s，线程 %s\n' "$open_file_growth" "$tcp_growth" "$thread_growth"
  printf -- '- 残留上限：Agent 子进程 %s，会话目录 %s\n' "$max_agent_children" "$max_session_directories"
  printf -- '- Gateway 健康失败：%s\n' "$gateway_failures"
  printf -- '- 安全扫描：不安全审计行 %s，疑似含密钥日志文件 %s\n' "$unsafe_audit_rows" "$secret_match_files"
} > "$summary_markdown_path"

printf 'stability complete passed=%s report=%s\n' "$passed" "$summary_json_path"
[ "$passed" = true ]
