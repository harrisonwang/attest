# attest — 路线图

> 状态：立项方案 · 2026-07-09
> 原则：每个阶段有明确交付物与 **kill criteria**——数据不支持就停或改，不带感情续命。
> 竞态提醒：agents-lint（见 00-vision §4.1）2026-03 已发 v0.5，窗口以月计。P0–P2 合计控制在 8 周内。

## P0 · 语料与假设验证（第 1–2 周）

先证明值得做，再写产品代码。

**做什么**

1. 挖矿脚本：扫 git 历史，找"commit A 改代码 → 后续 commit B 修文档"的配对，输出带标注的真实 drift 案例（doc 位置、错误 token、修正后 token、间隔时长）。
2. 语料源：自有仓库（spoor / vouch / answer-trace / jadeenvoy / kortiv …）+ 15–20 个知名开源仓库（有 CLAUDE.md/AGENTS.md 者优先）。目标 **200+ 案例**。
3. 人工归类每例落在哪个 resolver 类别（path / script / cmd / symbol / env / config-key / 散文），得出真实频率分布。
4. 把 [docs/probe/drift-probe.py](probe/drift-probe.py) 按已知缺陷修一轮（行内命令解析、doc-dir 基目录、只认 bash fence），在语料上测覆盖率与误报。

**交付**：`corpus/`（案例库，入库）+ 一页数据报告（分布、覆盖率、误报清单）。

**Kill criteria**：金档 resolver 类别合计覆盖历史真实 drift 案例 **< 60%** → 停下重新设计（说明散文占比超预期，架构要向银档前移）；探针修复版在语料上仍有无法归因到"缺 resolver"的系统性误报 → 公理 1 不成立，方案作废。

## P1 · MVP（第 3–5 周）

**做什么**

1. `attest-core` + `attest-cli` 骨架（Rust，vouch 同构，见 01-design §4）。
2. 提取器（pulldown-cmark + 命令行解析）+ resolver v1 前 6 个：path、script、pkg、cmd、env、symbol（go-import / config-key 视 P0 频率数据决定是否进 v1）。
3. verdict + 语境守卫 + 基线棘轮 + `--since` 增量。
4. 报告三渲染：TTY、`--format json`（attest.report.v1）、`--format github`（annotations）。
5. golden 测试全覆盖 + 语料回归跑进 CI + self-host。

**交付**：`attest check` 在自有全部仓库上可用，语料回归金档误报 = 0。

**验收**：对 P0 语料，MVP 检出率 ≥ 探针修复版；对 5 个从未见过的开源仓库冷启动运行，人工复核全部 broken 无一误报。

**Kill criteria**：为压误报不得不把主力 resolver 关到覆盖率 < 40% → 回 P0 重看数据。

## P2 · 分发与 launch（第 6–8 周）

**做什么**

1. 渠道四件套：`npx @harrisonwang/attest`（npm 包装二进制，零安装试用）、GitHub Action（`harrisonwang/attest-action`）、Claude Code skill（`/attest` 体检 + SessionStart hook 配方）、brew/scoop（复用 spoor tap）。
2. 扫 top-50 开源仓库（AGENTS.md 采用者名单可从 60k 存量里挑高星者），产出结构化发现报告。
3. 给其中金档确认、修复直白的 drift 发上游 PR（**只发金档**，每个 PR 都是带工具名的背书曝光；一个误报都不能有）。
4. Launch 内容："我们扫了 50 个知名仓库的 agent 文档，X% 在对 agent 撒谎"+ 方法论 + 工具。HN / X / 中文社区各一发。

**交付**：可安装的 v0.1、launch post、≥10 个上游 PR。

**验收信号**（决定 P3 投入力度）：安装量 / stars / PR 合并率 / 有无自来水 issue。没有硬门槛——但若 launch 后 30 天内无任何有机使用信号，P3 降速为兴趣项目节奏，主力回 spoor。

## P3 · lock 与银档（第 9–12 周，有 P2 信号后）

1. `.attest/claims.lock`：形状候选 + 绑定确认（纯机械档先行）。
2. `attest extract`：LLM 辅助散文断言抽取（OpenAI 兼容配置，与现有项目同款），锚点必须绑定成功才落盘，`proposed` → git review → `approved`。
3. 哈希锚点复查 + "锚点已变更"suspect 流。
4. vouch IR 集成：IR 声明的变更面 → 定向触发相关绑定复查（增量模式的精确版）。
5. `/attest-fix` skill：报告 → agent 改文档 → 复跑到绿——"文档自维护" demo 视频，第二波传播素材。

## P4 · 生态纵深（第 13 周起，按需求牵引排序）

- tree-sitter 符号索引（TS/Python/Rust/Go）替换 grep symbol，只收紧不翻案
- go-import / 路由 resolver（gin/echo/axum 路由注册表）/ config-key 深化
- 文档面扩展：README、docs/ 站点、mdbook/docusaurus 源
- introspective 档（`--allow-exec=repo-bins` 解析自产 CLI 的 --help）
- MCP server（沙箱化 agent 场景）、VS Code 诊断
- 工具表社区化（数据文件 PR 通道）

## 不做清单（防走形）

- 不做文档生成（scaffold-docs 的事；attest 只当裁判）
- 不做散文风格 lint、结构打分（AgentLinter 们的地盘，且必然主观）
- 不做 URL 检查（lychee 已解决，最多做集成）
- 不做通用测试框架；不在 CI 里调用任何 LLM（公理 3 是产品承诺，写进 README 第一屏）
- API 面守住：`check` / `baseline` / `extract` 三个子命令封顶到 P4

## 成功指标

| 维度 | 指标 | 目标 |
|------|------|------|
| 精度 | 语料回归金档误报 | 恒为 0（发布门禁） |
| 覆盖 | 挖矿语料检出率 | P1 ≥60% → P3 ≥80% |
| 采纳 | launch 后 90 天有机安装/stars/被写进别人 CI | 有明确自来水即算成立 |
| 声誉 | 上游 PR 合并率 | ≥70%（低于此说明金档纪律失守） |
