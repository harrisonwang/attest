# Reviewed upstream candidates

> Re-reviewed 2026-07-14 against the commit-pinned top-50 scan. Apply-checked diffs are stored in `reports/upstream-patches/`, and final titles, bodies, and PR URLs are stored in `reports/upstream-submissions.json`. Every live head is one clean signed-off commit on top of its submitted upstream base.

## Gold queue

| Repository | Snapshot | Gold evidence | Minimal upstream scope | Status |
|---|---|---|---|---|
| `react/react` | `c0c39a6b3907` | `compiler-port` names missing `hir.rs` and `reactive_function.rs`; current HIR definitions live in `lib.rs`/`reactive.rs`, and reactive-function construction lives in `react_compiler_reactive_scopes` | Refresh the Rust reading list | [PR #37008](https://github.com/react/react/pull/37008) |
| `oven-sh/bun` | `16c557635bb3` | Root `AGENTS.md` and `CLAUDE.md` still name `src/shell/`, `src/bake/`, and `src/http/websocket_client/`; current paths are `src/runtime/shell/`, `src/runtime/bake/`, and `src/http_jsc/websocket_client/` | Update the three moved subsystem paths in the mirrored file | [PR #34135](https://github.com/oven-sh/bun/pull/34135) |
| `OpenHands/OpenHands` | `5f9906fbdac3` | Agent docs retain a removed VS Code extension, retired action-handling symbols, and removed frontend model arrays; `action-type.tsx` moved, verified models are SDK/backend-owned, and repository instructions load from `.agents/skills/`, `.openhands/microagents/`, or legacy `.openhands/skills/` | Remove retired extension/action guidance and document current model/instruction sources | [PR #15262](https://github.com/OpenHands/OpenHands/pull/15262) |
| `continuedev/continue` | `d0a3c0b626b5` | `extensions/cli/AGENTS.md` still describes removed Hub authentication files and `src/mcp.ts`; current compatibility auth and MCP services live under `src/services/` plus `src/auth/workos.ts` | Refresh authentication and MCP architecture paths | [PR #12985](https://github.com/continuedev/continue/pull/12985) |
| `vercel-labs/skills` | `cf4a3ea678b7` | Agent docs prescribe `pnpm run -C scripts ...`, but `scripts/` has no package manifest and the root project executes TypeScript directly with Node | Replace validate/sync commands with `node scripts/*.ts` | [PR #1681](https://github.com/vercel-labs/skills/pull/1681) |
| `alirezarezvani/claude-skills` | `8c4a374a443a` | `.claude/commands/README.md` names lowercase `.github/pull_request_template.md`; the tracked file is `.github/PULL_REQUEST_TEMPLATE.md` | Correct one path’s casing | [PR #914](https://github.com/alirezarezvani/claude-skills/pull/914) |
| `deanpeters/Product-Manager-Skills` | `99be43c842d3` | `CLAUDE.md` names two absent local Product Porch transcript files while already linking the canonical podcast episodes | Remove the stale local transcript inventory | [PR #21](https://github.com/deanpeters/Product-Manager-Skills/pull/21) |
| `jeremylongshore/claude-code-plugins-plus-skills` | `2d86dfbcf3e2` | Firebase docs use a removed validator and an incorrect repository traversal; the authoritative validator is `scripts/validate-skills-schema.py` | Replace all three stale Firebase validation commands and fix the root traversal | [PR #1055](https://github.com/jeremylongshore/claude-code-plugins-plus-skills/pull/1055) |
| `NVIDIA/skills` | `9559272b38d9` | DICOM agent instructions prescribe `make run-skill ...`, but the repository has no Makefile or `run-skill` target; the fixture generator and extractor are tracked | Document the repository-contained generate-and-extract flow | [PR #344](https://github.com/NVIDIA/skills/pull/344) |
| `OpenHands/docs` | `a7d418214914` | Root `AGENTS.md` names `openapi/openapi.json`; the current REST schema is `openapi/V0_openapi.json` | Correct one OpenAPI path | [PR #621](https://github.com/OpenHands/docs/pull/621) |

## Rejected findings

- `badlogic/pi-mono` fixed `image-limits.test.ts` upstream before submission and is no longer eligible.
- Open Pencil’s `packages/render/` and `packages/linter/` references explicitly describe the external `figma-use` repository; they are not local drift.
- The Mnemos missing-resource links in the Jeremy Longshore scan originate in the `polyxmedia/mnemos` mirror. Repository policy requires a friendly issue and fix in that source repository before sync, so the aggregator patch does not edit them.
- Generated outputs, filename templates, logical skill identifiers, external SDK examples, and target-project reference docs remain suspect or silent rather than hard failures.
- Only the ten rows above were eligible for branded upstream PRs; all ten now have real public PR URLs.

Validate one pinned clone and print its reviewed submission copy with:

```bash
python3 tools/prepare_upstream_pr.py owner/repo --clone /path/to/clone
```

Add `--apply` only after reviewing the pinned head and confirming the clone is disposable or dedicated to that PR.
