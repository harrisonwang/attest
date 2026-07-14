# Launch draft

## Headline

We scanned 50 popular open-source agent instruction sets. 9 contained at least one deterministic missing binding candidate.

## Long post

Agent instructions are executable documentation. A stale path, renamed script, or removed package does not merely confuse a reader; it sends an autonomous tool down the wrong branch.

We built **attest**, a zero-network static checker for `AGENTS.md`, `CLAUDE.md`, and `.claude/**/*.md`. It extracts concrete code literals, binds them to repository facts, and reports only four outcomes: verified, broken, suspect, or silent. Ambiguous, generated, hypothetical, external, and example-only references never fail CI.

For the launch scan we ranked public GitHub repositories by stars and successfully scanned **50/50 repositories**, covering **330 documents** and **16,310 tokens**. Attest deterministically verified **8,201 bindings**. After context guards, **9 repositories (18%)** still had at least one automatic broken candidate, totaling **19 findings**. Suspect findings remain advisory and are not counted in that headline.

The raw number is not an excuse to spray automated PRs. We manually reviewed low-noise findings against pinned public commits and prepared ten minimal patches that apply cleanly to those snapshots. Generated files, templates, external checkouts, local state, placeholders, and multilingual examples were explicitly rejected. Every branded upstream PR must come from that reviewed gold queue.

Attest is designed for brownfield adoption: `attest baseline update` records existing debt, while CI fails only on new broken bindings. JSON output is self-contained for agents, GitHub output emits annotations, and an optional author-time LLM extractor can propose prose claims only when every anchor binds deterministically.

Install and run:

```bash
npx @harrisonwang/attest check
# or use the native release binary
attest check --format github
```

Methodology and reproducible outputs live in `reports/public-scan.json`, `reports/public-scan.md`, `reports/reviewed-findings.md`, `reports/upstream-patches/`, and `corpus/reviewed.jsonl`.

## Hacker News

**Title:** Show HN: Attest — regression tests for AGENTS.md and CLAUDE.md

**Text:** We scanned 50 popular public repositories and found deterministic missing-binding candidates in 9 after conservative context guards. Attest is a Rust CLI that checks paths, scripts, workspace packages, commands, Go imports, environment variables, config keys, and symbols without executing repository code. Existing debt can be baselined; only new broken bindings fail CI. The corpus and structured scan report are published with the source.

## X thread

1. Agent docs are executable docs. If `AGENTS.md` points at a removed file, your coding agent starts with a false premise.
2. We built attest: deterministic bindings for paths, scripts, packages, commands, imports, env vars, config keys, and symbols. No repository code execution.
3. Public launch scan: 50/50 repos, 330 docs, 16,310 tokens, 8,201 verified bindings. 9 repos retained at least one broken candidate after conservative guards.
4. We do not auto-file noisy PRs. Ten low-noise patches were manually checked and apply-tested against pinned commits; templates, generated outputs, placeholders, and external paths were rejected.
5. Brownfield-friendly: baseline existing debt, fail only on new breakage. `npx @harrisonwang/attest check`.

## 中文社区

Agent 文档已经是“可执行文档”：路径、脚本或包名一旦过期，agent 会从错误前提出发。

我们做了 **attest**，用确定性 resolver 检查 `AGENTS.md`、`CLAUDE.md` 和 `.claude/**/*.md`，不执行仓库代码。公开 launch 扫描成功覆盖 50 个高星仓库、330 份文档和 16,310 个 token，确定性验证 8,201 个绑定；经过保守语境守卫后，9 个仓库仍至少有一个自动 broken 候选，共 19 条。

这个数字不会被拿来批量轰炸上游。我们只从人工复核且已通过快照应用检查的 10 个金档补丁发 PR，生成文件、模板、外部 checkout、本地状态、占位符和多语言示例全部剔除。老项目可以先 `attest baseline update`，CI 只拦新增 drift。
