# attest

**文档回归测试。文档里声明的，仓库里必须成立。**

`attest` 从 Markdown 的 inline code 与显式 shell fence 中机械提取 token，再绑定到仓库里的路径、脚本、workspace 包、命令、Go import、环境变量、配置键和源码符号。CI 内不调用 LLM、不执行仓库代码；不确定项只会成为 `suspect` 或保持沉默，只有确定失效的绑定才是 `broken`。

## 安装

```bash
cargo install --path crates/attest-cli
# 或零安装运行发布版
npx @harrisonwang/attest check
# macOS / Linux
brew install harrisonwang/tap/attest
# Windows
scoop bucket add harrisonwang https://github.com/harrisonwang/scoop-bucket
scoop install attest
```

## 使用

```bash
# 默认检查 CLAUDE.md、AGENTS.md、SKILL.md 与 .claude/**/*.md
attest check

# 指定文档与输出格式
attest check README.md --format json
attest check --format github
attest check --strict  # 裸路径形状无法绑定时给 suspect

# brownfield 仓库先记录存量问题，此后只为新增 broken 失败
attest baseline update
attest check

# 只针对相对分支发生的变更检查
attest check --since origin/main

# 用 vouch Commit IR 的变更面定向复查相关文档
attest check --vouch-ir .vouch/commit-ir.json

# 从散文中的路径/环境变量形状生成可审阅的 proposed claims
attest extract

# 作者时点可选 LLM 抽取；每个模型锚点仍须确定性绑定才会写入
OPENAI_API_KEY=... attest extract --llm
```

退出码：`0` 表示没有新增 broken，`1` 表示发现新增 broken，`2` 表示配置、输入或运行错误。

`attest extract` 默认只写入能够当场绑定成功的机械候选到 `.attest/claims.lock`。`--llm` 使用作者时点的 OpenAI-compatible Responses API + Structured Outputs；可通过 `ATTEST_OPENAI_MODEL`（默认 `gpt-5.6-terra`）与 `OPENAI_BASE_URL` 配置。模型提出的任一锚点无法确定性绑定时，整条 claim 都不会生成。将 `status: proposed` 审阅为 `approved` 后，`check` 会复查来源文档和锚点：任一删除都是 broken，内容哈希变化只是 suspect；lock 的未知字段、空锚点和非法哈希会作为输入错误拒绝。

GitHub Actions 可在 checkout 后使用本仓库的 `action.yml`；默认输出 PR annotations。Claude Code 的 `/attest` skill 与可选 SessionStart hook 配方位于 `.claude/skills/attest/`。

公开 top-50 扫描、五仓冷启动验收、候选频率语料、launch 草稿、10 个可应用上游补丁、人工复核队列、65 秒修复 demo 与逐项完成审计见 `reports/public-scan.md`、`reports/cold-start-validation.md`、`corpus/report.md`、`reports/launch-post.md`、`reports/upstream-patches/`、`reports/reviewed-findings.md`、`demo/attest-fix/attest-fix.mp4` 和 `reports/completion-audit.md`。

## 配置

所有配置均可省略。仓库根目录的 `attest.toml` 示例：

```toml
[docs]
include = ["CLAUDE.md", "AGENTS.md", "**/SKILL.md", "docs/**/*.md"]
exclude = ["**/node_modules/**"]

[resolvers]
symbol = true
go-import = true

[policy]
fail-on = "broken"
context-guard = true

[scope]
"docs/ops/**" = "repo-root"
```

## 安全模型

- 默认档只读取文件系统、manifest、vendored 工具表和 Git 元数据。
- 文档中的命令永远不会被执行。
- LLM 不进入 `check` 或 CI 路径，结果可复现。
- 只有显式 `extract --llm` 才访问网络；API key 不写入 lock 或报告。
- 近失、歧义与否定/假设语境降级为 `suspect`，不会让 CI 失败。

设计、精度纪律和路线图见 `docs/README.md`。
