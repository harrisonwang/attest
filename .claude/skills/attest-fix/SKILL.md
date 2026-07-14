---
name: attest-fix
description: Repair confirmed repository documentation drift reported by attest and rerun checks to green. Use when stale paths, scripts, package names, commands, environment variables, configuration keys, or symbols must be corrected in Markdown.
---

# Attest Fix

Fix documentation against repository facts; do not change code merely to satisfy stale prose.

## Workflow

1. Run `attest check --format json` from the repository root, or `cargo run -p attest-cli -- check --format json` when developing attest itself.
2. Fix every unbaselined `broken` finding in its source document. Verify suggestions against the current repository before applying them.
3. Review `suspect` findings manually. Change them only when repository evidence confirms drift.
4. Preserve the document's intent, language, formatting, and surrounding instructions. Make the smallest factual edit.
5. Rerun the same check after edits. Continue until no unbaselined `broken` findings remain.
6. Summarize changed documents and final counts. Never hide failures with `baseline update`.
