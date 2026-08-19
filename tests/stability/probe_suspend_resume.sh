#!/usr/bin/env bash

set -u
set -o pipefail
umask 077

usage() {
  printf '%s\n' \
    "Usage: probe_suspend_resume.sh [options]" \
    "" \
    "Options:" \
    "  --pid PID              Existing HAL100 desktop PID (auto-detected by default)" \
    "  --pause-seconds N      Process suspension duration (default: 5)" \
    "  --output-dir PATH      Report directory" \
    "  --gateway-url URL      Health endpoint (default: http://127.0.0.1:10100/healthz)" \
    "  --help                 Show this help"
}

is_positive_integer() {
  case "$1" in
    ''|*[!0-9]*|0) return 1 ;;
    *) return 0 ;;
  esac
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
hal100_pid=""
pause_seconds=5
gateway_url="http://127.0.0.1:10100/healthz"
output_dir=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --pid)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      hal100_pid=$2
      shift 2
      ;;
    --pause-seconds)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      pause_seconds=$2
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

is_positive_integer "$pause_seconds" || {
  printf 'pause duration must be a positive integer\n' >&2
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
  output_dir="$workspace_root/output/stability/suspend-resume-$run_stamp"
fi
mkdir -p "$output_dir"
chmod 700 "$output_dir"
report_path="$output_dir/summary.md"

continued=0
ensure_continued() {
  if [ "$continued" -eq 0 ]; then
    kill -CONT "$hal100_pid" 2>/dev/null || true
    continued=1
  fi
}
trap ensure_continued EXIT INT TERM

curl --fail --silent --show-error --max-time 2 "$gateway_url" >/dev/null || {
  printf 'Gateway was not healthy before suspension\n' >&2
  exit 1
}
agent_children_before=$(ps -axo ppid=,command= | awk -v pid="$hal100_pid" '
  $1 == pid && ($0 ~ /agent-kernel\/dist\/index/ || $0 ~ /llama-server/ || $0 ~ /hal100-agent/) { count += 1 }
  END { print count + 0 }
')
[ "$agent_children_before" -eq 0 ] || {
  printf 'Agent or model child process is active; suspend/resume probe requires idle HAL100\n' >&2
  exit 1
}

started_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
kill -STOP "$hal100_pid"
sleep 1
process_state=$(ps -p "$hal100_pid" -o state= | tr -d ' ')
case "$process_state" in
  *T*) ;;
  *)
    printf 'HAL100 did not enter the suspended process state: %s\n' "$process_state" >&2
    exit 1
    ;;
esac

if [ "$pause_seconds" -gt 1 ]; then
  sleep $((pause_seconds - 1))
fi
ensure_continued

recovered=0
attempt=0
while [ "$attempt" -lt 20 ]; do
  attempt=$((attempt + 1))
  if curl --fail --silent --show-error --max-time 1 "$gateway_url" >/dev/null 2>&1; then
    recovered=1
    break
  fi
  sleep 1
done

agent_children_after=$(ps -axo ppid=,command= | awk -v pid="$hal100_pid" '
  $1 == pid && ($0 ~ /agent-kernel\/dist\/index/ || $0 ~ /llama-server/ || $0 ~ /hal100-agent/) { count += 1 }
  END { print count + 0 }
')
completed_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
passed=false
if [ "$recovered" -eq 1 ] && [ "$agent_children_after" -eq 0 ] && kill -0 "$hal100_pid" 2>/dev/null; then
  passed=true
fi

{
  printf '# HAL100 进程暂停/恢复探针\n\n'
  printf -- '- 结果：%s\n' "$passed"
  printf -- '- 时间：%s — %s\n' "$started_utc" "$completed_utc"
  printf -- '- PID：%s\n' "$hal100_pid"
  printf -- '- 暂停时长：%s 秒\n' "$pause_seconds"
  printf -- '- 暂停状态：%s\n' "$process_state"
  printf -- '- Gateway 恢复：%s（最多等待 20 秒）\n' "$recovered"
  printf -- '- 暂停前 Agent/模型子进程：%s\n' "$agent_children_before"
  printf -- '- 恢复后 Agent/模型子进程：%s\n' "$agent_children_after"
  printf '\n此探针只模拟进程被系统调度暂停后继续，不等同于整机睡眠/唤醒；真机步骤见内部测试说明。\n'
} > "$report_path"

printf 'suspend-resume complete passed=%s report=%s\n' "$passed" "$report_path"
[ "$passed" = true ]
