# Completion audit

> Audited 2026-07-14 against `docs/02-roadmap.md`. “Proven” means the named artifact and its relevant gate were inspected or executed; drafts are not counted as external delivery.

## Requirement ledger

| Phase | Requirement | Evidence | State |
|---|---|---|---|
| P0 | Mining tool, public reviewed corpus, and category distribution | `tools/mine-drift.py`; 280 Git-snapshot-verified cases from 15 public repositories in `corpus/reviewed.jsonl`; 1,464 digest-bound frequency candidates from 10 deep public histories in `corpus/candidates.jsonl` | proven |
| P0 | Coverage gate | Rust corpus test requires every corrected path to bind and at least 80% of stale paths to be broken or suspect | proven |
| P1 | Core, CLI, extraction, eight resolvers, verdicts, baseline, incremental mode, three reports | workspace sources and 52 Rust tests | proven |
| P1 | Owned-repository compatibility | release binary scanned 30 local owned repositories: 27 clean, 3 with expected drift exit code, 0 runtime errors; privacy-preserving machine evidence in `reports/owned-repository-validation.json` | proven locally |
| P1 | Five-repository cold start with zero false-positive broken findings | `reports/cold-start-validation.md` and matching machine report evidence | proven |
| P2 | npm wrapper, native releases, Action, skills, brew and Scoop assets | `packages/npm/`, `.github/workflows/release.yml`, `distribution/attest-action/`, `.claude/`, channel templates, release preflight, and distribution parity tests | proven locally |
| P2 | Top-50 public scan and structured report | 50/50 successful repositories, 330 docs and 16,310 tokens in `reports/public-scan.json` | proven |
| P2 | Ten gold upstream candidate patches, submission copy, and launch copy | `reports/upstream-patches/`, `reports/upstream-submissions.json`, `reports/reviewed-findings.md`, and `reports/launch-post.md` | prepared, not externally delivered |
| P2 | Installable v0.1, independent Action repository, at least ten submitted PRs, HN/X/Chinese launch | requires GitHub/npm credentials and public side effects | not delivered |
| P3 | Strict claims lock, deterministic extraction, author-time Structured Outputs, hashes, vouch targeting, fix skill | source, tests, `.claude/skills/attest-fix/` | proven locally |
| P3 | Recorded repair demo | executable red-to-green fixture, asciinema v2 event stream, and 65-second H.264 video in `demo/attest-fix/` | proven locally |
| P3 | Second-wave launch | finalized recording and launch copy are ready, but public posting requires external execution | not externally delivered |
| P4 | Demand-driven ecosystem items | explicitly not a v0.1 gate in the roadmap/status | not required for current release |

## Release gates

The current worktree passes:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release --locked -p attest-cli`
- `python3 tools/validate_release.py --tag v0.1.0`
- self-hosted `attest check` with 0 broken findings
- executable `demo/attest-fix/run.sh` red-to-green check
- no-video replay of the committed demo recorder against the release binary
- 23 Python corpus/report/distribution tests
- npm installer unit checks, install bypass, launcher, SessionStart hook, and `npm pack --dry-run`
- authenticated `npm publish --dry-run --access public` for the four-file `@harrisonwang/attest@0.1.0` package

## External blocker

The workspace now has a fresh `main` Git repository and both GitHub CLI and npm are authenticated as `harrisonwang`, but the repository has no initial commit or remote and the target `harrisonwang/attest` and `harrisonwang/attest-action` repositories do not yet exist. An authenticated public npm publish dry run succeeds, while `@harrisonwang/attest` remains unpublished. Consequently the roadmap’s public P2/P3 side effects cannot be truthfully marked complete yet. Creating commits, public repositories, packages, pull requests, or launch posts requires explicit publication approval; the exact bootstrap and steady-state process is documented in `docs/release.md`.
