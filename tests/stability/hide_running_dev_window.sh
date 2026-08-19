#!/usr/bin/env bash

set -u
set -o pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
binary_path="$workspace_root/target/debug/hal100-desktop"

mapfile_supported=0
if command -v mapfile >/dev/null 2>&1; then
  mapfile_supported=1
fi

if [ "$mapfile_supported" -eq 1 ]; then
  mapfile -t matching_pids < <(pgrep -f "$binary_path" || true)
else
  matching_pids=()
  while IFS= read -r pid; do
    [ -n "$pid" ] && matching_pids+=("$pid")
  done < <(pgrep -f "$binary_path" || true)
fi

if [ "${#matching_pids[@]}" -ne 1 ]; then
  printf 'Expected exactly one running HAL100 development process, found %s\n' \
    "${#matching_pids[@]}" >&2
  exit 2
fi

process_command=$(ps -p "${matching_pids[0]}" -o command= 2>/dev/null || true)
if [ "$process_command" != "$binary_path" ]; then
  printf 'Refusing to target unexpected process: %s\n' "$process_command" >&2
  exit 2
fi
if [ ! -x "$binary_path" ]; then
  printf 'HAL100 development binary is unavailable: %s\n' "$binary_path" >&2
  exit 2
fi

"$binary_path" --hal100-dev-hide-window
printf 'HAL100 development window hide request sent to PID %s\n' "${matching_pids[0]}"
