# attest — 实现状态

> 更新：2026-07-14

## 已实现

- Rust workspace：`attest-core` 保持 I/O-behind-trait，`attest-cli` 按职责分成 check / extract / llm / vouch / render / store / surface / prose 等模块，main 只做参数解析和分发。
- CommonMark 提取：inline code 与显式 bash/sh/shell/zsh/console fence，共用命令解析器；支持环境前缀、注释、续行、heredoc 跳过，以及路径/script/pkg 的 `*` / `?` 确定性通配绑定（无匹配时静默）。已知盲区：无语言标注的 fence 不产 token，这类内容里的漂移工具看不见。
- v1 resolver：path、script、pkg、cmd、go-import、env、config-key、symbol；工具子命令表 vendored 为 JSON 数据。表内工具的未知子命令保持沉默，不算 verified；owner/repo 形状的仓库缩写不做迁移猜测。
- 事实采集：Git 文件树尊重 ignore；符号/env 查询首次调用建一遍词表索引，之后查表；文档引用命中 .gitignore 的路径走 `git check-ignore`（带缓存）按运行时产物降级；config-key 仅绑定上下文点名或 doc/project 邻近配置，避免无关配置假绿。
- monorepo scope：doc-dir → project-root → repo-root，支持 `attest.toml` glob 钉死作用域；路径 glob 在 core 里只有一份实现，CLI 与测试替身共用。
- verdict：verified / broken / suspect / silent。broken 报出前过四道降级守卫（结构 / 语境 / 文档类别 / 形状），只看 token 所在行、先遮掉行内代码再匹配，每道守卫的降级理由写进 evidence.note；守卫行为由 `corpus/guard-cases.jsonl` 带标注语料逐条回归，仓库特有的例外只进语料不进代码。改法建议只在唯一候选时给出，多候选只列在 evidence 里。
- CLI：`check`、`baseline update`、`extract`，支持 TTY / JSON / GitHub annotations、`--since`、`--vouch-ir`、`--strict`。
- brownfield 棘轮：`.attest/baseline.json` 按 `(doc, token, ns)` 稳定匹配。
- 银档：严格 YAML `.attest/claims.lock`、proposed/approved、确定性锚点、SHA-256 内容锚定；来源文档或锚点删除报 broken，内容变化只报 suspect。
- 作者时点 LLM 抽取：OpenAI-compatible Responses API、严格 Structured Outputs、模型/端点可配置；全部锚点确定性绑定成功才生成 proposed claim。
- vouch IR 定向模式：读取 Commit IR 的文件与 anchor 变更面，只复查直接变更或提及相关锚点的文档及其 lock claims。
- 分发资产：npm 零安装包装器与六平台目标清单、composite GitHub Action 及独立仓库导出包、六平台 release workflow、版本/tag fail-closed 预检、npm OIDC trusted publishing、Homebrew/Scoop 模板与自动 dispatch、`attest` / `attest-fix` agent skills、SessionStart hook 配方。
- 语料：公开 Git 快照公证案例与 CI corpus gate；挖矿工具支持批量仓库发现、隐私过滤和 reviewed-only 输出。
- P0 已收录 280 条、覆盖 15 个公开仓库的 Git 快照确认案例，满足 200+ 案例和 15–20 来源仓库门槛；另有 10 个公开深历史仓库的 1,464 条 digest-bound 机械候选用于 resolver 频率分析，候选不混入精度金档。未使用私有或合成数据填数。检出门禁分两条线：broken 率（CI 真会拦住的）≥55%，broken+suspect 总检出 ≥80%；当前实测 broken 58.6%、总检出 85.0%。
- P1 冷启动验收：五个未进入语料和 top-50 launch 扫描的公开仓库共 9 条 broken 已逐条复核，金档误报为 0；证据见 `reports/cold-start-validation.md`。
- P1 自有仓库兼容性：release binary 扫描同级 30 个 Git 仓库，27 个 clean、3 个按产品语义返回 drift、0 个运行错误；聚合证据不记录私有仓库身份，见 `reports/owned-repository-validation.md`。
- P2 扫描：按 GitHub stars 排序并成功扫描 50/50 个公开 AGENTS/CLAUDE 仓库，共覆盖 330 份文档和 16,310 个 token；结构化报告、launch 草稿和 10 个已通过 `git apply --check` 的金档上游补丁均已落盘。
- P3 修复闭环：`/attest-fix` skill、临时 Git 仓库红→绿夹具、asciinema 事件流与 65 秒 H.264 demo 已落盘；夹具和录制证据均进入 CI。
- 工具表社区通道：第三方命令保持纯 JSON 数据，贡献规范要求官方证据、排序、golden test 与完整门禁。

## 尚需外部执行

- P2 的 npm/GitHub Release 发布、将 `distribution/attest-action/` 推送为独立仓库、10 个上游 PR 提交以及 HN/X/中文社区发帖仍需要真实对外执行；GitHub CLI 与 npm 均已认证为 `harrisonwang`，`npm publish --dry-run --access public` 已通过，但主仓库/Action 仓库尚未创建，npm 包也尚未公开。精确引导见 `docs/release.md`。
- P3 demo 已真实录制；第二波发布仍属于对外传播执行。
- P4 项目按需求牵引，不算 v0.1 门禁：tree-sitter、路由 resolver、MCP、VS Code 与 introspective 执行档。

逐项证据与未完成判定见 `reports/completion-audit.md`。

## 验收命令

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p attest-cli -- check
python3 tools/validate_release.py
python3 -m unittest discover -s tools -p 'test_*.py'
npm pack --dry-run
```
