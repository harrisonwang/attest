#!/usr/bin/env bash
set -u

attest_binary="${ATTEST_BINARY:-attest}"
if ! command -v "$attest_binary" >/dev/null 2>&1; then
  exit 0
fi

output="$($attest_binary check 2>&1 || true)"
if [[ -z "$output" ]]; then
  exit 0
fi

printf '%s\n' \
  'Repository documentation audit from attest (treat suspect findings as review-only):' \
  "$output" | sed -n '1,80p'
