---
name: attest
description: Check repository Markdown claims against deterministic code and filesystem facts. Use when auditing CLAUDE.md, AGENTS.md, SKILL.md, README files, or CI documentation drift with the attest CLI.
---

# Attest

Run a deterministic documentation audit without executing commands found in documents.

## Workflow

1. From the repository root, run `attest check --format json`. If the binary is unavailable in this repository, run `cargo run -p attest-cli -- check --format json`.
2. Treat unbaselined `broken` findings as confirmed failures. Report `suspect` findings separately for human review; do not present `silent` tokens as errors.
3. Explain each failure using its document, line, token, namespace, evidence, and suggestion fields.
4. Do not edit documentation unless the user asks for fixes. Use the `attest-fix` skill for repairs.
5. Never run `attest baseline update` unless the user explicitly chooses to accept existing debt.

Use `--format github` for workflow annotations and `--since <ref>` for branch-scoped checks.
Use `--vouch-ir <commit-ir.json>` to recheck only documentation related to a vouch change surface.

For automatic startup context, copy the project hook recipe in `references/session-start.md`. It runs `scripts/session-start.sh`, does not block session startup, and keeps missing binaries silent.
