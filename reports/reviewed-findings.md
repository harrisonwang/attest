# Reviewed upstream candidates

> Re-reviewed 2026-07-14 against the commit-pinned top-50 scan. Apply-checked diffs are stored in `reports/upstream-patches/`, and copy-ready titles/bodies are stored in `reports/upstream-submissions.json`; they are not submitted pull requests. Each head must still be rechecked immediately before submission.

## Gold queue

| Repository | Snapshot | Gold evidence | Minimal upstream scope | Status |
|---|---|---|---|---|
| `facebook/react` | `c0c39a6b3907` | `compiler-port` names missing `hir.rs` and `reactive_function.rs`; current HIR definitions live in `lib.rs`/`reactive.rs`, and reactive-function construction lives in `react_compiler_reactive_scopes` | Refresh the Rust reading list | patch exported |
| `oven-sh/bun` | `5098c8dada2f` | Root `AGENTS.md` and `CLAUDE.md` still name `src/shell/`, `src/bake/`, and `src/http/websocket_client/`; current paths are `src/runtime/shell/`, `src/runtime/bake/`, and `src/http_jsc/websocket_client/` | Update the three moved subsystem paths in the mirrored file | patch exported; suspect manually confirmed |
| `OpenHands/OpenHands` | `5f9906fbdac3` | Agent docs retain a removed VS Code extension, retired action-handling symbols, and removed frontend model arrays; `action-type.tsx` moved, verified models are SDK/backend-owned, and repository instructions load from `.agents/skills/`, `.openhands/microagents/`, or legacy `.openhands/skills/` | Remove retired extension/action guidance and document current model/instruction sources | patch exported |
| `continuedev/continue` | `c5490d97eaa9` | `extensions/cli/AGENTS.md` still names removed `src/mcp.ts`; MCP now lives in `src/services/MCPService.ts`, `mcpTransports.ts`, and `mcpUtils.ts` | Rewrite the MCP architecture bullet only | patch exported |
| `vercel-labs/skills` | `cf4a3ea678b7` | Agent docs prescribe `pnpm run -C scripts ...`, but `scripts/` has no package manifest and the root project executes TypeScript directly with Node | Replace validate/sync commands with `node scripts/*.ts` | patch exported |
| `alirezarezvani/claude-skills` | `0241f4376557` | `.claude/commands/README.md` names lowercase `.github/pull_request_template.md`; the tracked file is `.github/PULL_REQUEST_TEMPLATE.md` | Correct one path’s casing | patch exported |
| `deanpeters/Product-Manager-Skills` | `99be43c842d3` | `CLAUDE.md` names two absent local Product Porch transcript files while already linking the canonical podcast episodes | Remove the stale local transcript inventory | patch exported |
| `jeremylongshore/claude-code-plugins-plus-skills` | `7ca29e06dbfa` | Firebase docs use a removed validator and an incorrect repository traversal; the authoritative validator is `scripts/validate-skills-schema.py` | Replace all three stale Firebase validation commands and fix the root traversal | patch exported |
| `NVIDIA/skills` | `9559272b38d9` | DICOM agent instructions prescribe `make run-skill ...`, but the repository has no Makefile or `run-skill` target; the fixture generator and extractor are tracked | Document the repository-contained generate-and-extract flow | patch exported |
| `OpenHands/docs` | `a7d418214914` | Root `AGENTS.md` names `openapi/openapi.json`; the current REST schema is `openapi/V0_openapi.json` | Correct one OpenAPI path | patch exported |

## Rejected findings

- `badlogic/pi-mono` fixed `image-limits.test.ts` upstream before submission and is no longer eligible.
- Open Pencil’s `packages/render/` and `packages/linter/` references explicitly describe the external `figma-use` repository; they are not local drift.
- The Mnemos missing-resource links in the Jeremy Longshore scan originate in the `polyxmedia/mnemos` mirror. Repository policy requires a friendly issue and fix in that source repository before sync, so the aggregator patch does not edit them.
- Generated outputs, filename templates, logical skill identifiers, external SDK examples, and target-project reference docs remain suspect or silent rather than hard failures.
- Only the ten rows above are eligible for branded upstream PRs, and no row counts as delivered until a real PR URL exists.

Validate one pinned clone and print its reviewed submission copy with:

```bash
python3 tools/prepare_upstream_pr.py owner/repo --clone /path/to/clone
```

Add `--apply` only after reviewing the pinned head and confirming the clone is disposable or dedicated to that PR.
