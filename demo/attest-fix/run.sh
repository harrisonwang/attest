#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/../.." && pwd)"
attest_binary="${ATTEST_BINARY:-$project_root/target/release/attest}"
pause_seconds="${DEMO_PAUSE_SECONDS:-0}"
worktree="$(mktemp -d "${TMPDIR:-/tmp}/attest-fix-demo.XXXXXX")"

cleanup() {
  if [[ "${KEEP_ATTEST_DEMO:-0}" == "1" ]]; then
    printf 'Demo worktree retained at %s\n' "$worktree"
  else
    rm -rf "$worktree"
  fi
}
trap cleanup EXIT

test -x "$attest_binary"
mkdir -p "$worktree/src"
cp "$script_dir/AGENTS.stale.md" "$worktree/AGENTS.md"
cp "$script_dir/src/auth.rs" "$worktree/src/auth.rs"
git -C "$worktree" init -q
git -C "$worktree" add .
GIT_AUTHOR_DATE="2026-07-14T00:00:00Z" \
  GIT_COMMITTER_DATE="2026-07-14T00:00:00Z" \
  git -C "$worktree" -c user.name=attest-demo -c user.email=demo@example.invalid \
  commit -qm "demo fixture"

printf '\n$ cat AGENTS.md\n'
cat "$worktree/AGENTS.md"
sleep "$pause_seconds"

printf '\n$ attest check --format json\n'
set +e
first_report="$($attest_binary --root "$worktree" check AGENTS.md --format json)"
first_status=$?
set -e
printf '%s\n' "$first_report"
test "$first_status" -eq 1
FIRST_REPORT="$first_report" python3 - <<'PY'
import json
import os

report = json.loads(os.environ["FIRST_REPORT"])
assert report["stats"]["broken"] == 1
assert any(
    finding["token"] == "src/legacy_auth.rs" and finding["verdict"] == "broken"
    for finding in report["findings"]
)
PY
sleep "$pause_seconds"

printf '\n$ /attest-fix\n'
python3 - "$worktree/AGENTS.md" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
path.write_text(
    path.read_text(encoding="utf-8").replace("src/legacy_auth.rs", "src/auth.rs"),
    encoding="utf-8",
)
PY

printf '\n$ git diff -- AGENTS.md\n'
git -C "$worktree" diff -- AGENTS.md
sleep "$pause_seconds"

printf '\n$ attest check\n'
"$attest_binary" --root "$worktree" check AGENTS.md
