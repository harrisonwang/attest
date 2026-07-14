# attest — 定位与论题

> 状态：立项方案 · 2026-07-09
> 一句话：**文档回归测试。文档里声明的，仓库里必须成立。**

## 1. 问题

文档写完就开始过时。目录改名、脚本重命名、flag 删除、API 变更——代码在演进，文档不知道。人类没有能力（也没有意愿）持续对着代码同步维护文档，这件事在软件史上从未被解决，只是被容忍。

Agent 时代把"容忍"变成了"故障"。CLAUDE.md、AGENTS.md、SKILL.md 的读者不再是人，而是每个 session 都逐字照做的 agent：一条记载着已删除脚本的指令，意味着 agent 每次都撞墙、烧 token、然后带着错误前提继续工作。**文档从参考资料变成了承重的运行时构件**——而承重构件理应有回归测试。

外部信号确认这不是臆想：

- AGENTS.md 已由 Anthropic、OpenAI、Block 捐入 Linux Foundation（Agentic AI Foundation，2025 末），60,000+ 项目在用——标准化带来存量，存量带来腐烂。
- 2026 年已有研究测得：过时/劣质的 context 文件使任务成功率下降 2–3%，成本上升 20%+。
- 同类工具开始萌芽（见 §4），说明痛点已被多方独立感知——赛道正在形成，但没人做对架构。

## 2. 论题

attest 属于一个更大的论题：**模型负责叙述，机器负责公证。** 生成是模型的事，核验是基础设施的事；核验必须是确定性的，否则就是用一个幻觉去检查另一个幻觉。

这个论题在姊妹项目中已经三次落地：

| 项目 | 公证什么 | 对什么核验 |
|------|---------|-----------|
| answer-trace | LLM 回答里的事实断言 | 源文档（quote 必须逐字 locate 到） |
| vouch | commit 的意图声明（IR） | staged diff（声明的必须存在，存在的必须被声明） |
| git-why | AI 的改动解释 | 代码逐条核对，一处不符就不生成 |
| **attest** | **文档对仓库的指涉** | **仓库本身（token 必须绑定到命名空间）** |

模型能力越强、产出越多，"它说的还成立吗"这个问题的总量就越大。生成侧工具会被下一代模型吃掉；核验侧工具随模型变强而更被需要。这是独立开发者少有的、与巨头利益不冲突的顺风向。

## 3. 核心洞察（为什么这条路可行）

"文档没有格式规范 → 无法抽取 → 无法核验"的推理链，前提是自顶向下：先理解文档、把句子分类成断言、再核验。那条路确实是 NLU 无底洞。

attest 的设计是反转的——**分类靠绑定，不靠理解**：

1. 唯一依赖的"格式规范"是 CommonMark 本身：inline code span 与 fenced block 解析零歧义。开发者文档有一个近乎普遍的经验事实——可执行、可引用的东西都在反引号里，因为作者就是想让读者（人或 agent）照着敲。
2. 拿到 token 后不问"作者什么意思"，而是挨个命名空间尝试绑定：存在于文件树吗？是 package.json script 吗？是 workspace 包名吗？是源码符号吗？**仓库本身就是判定器（oracle）**。
3. 非对称性是整个产品能活的根本：**抽取失败的代价是沉默（少覆盖一条），永远不会因为"理解错句子"而误报**。核验的单位不是句子，是绑定。覆盖率可以从 60% 爬到 90%，精度从第一天就是硬的。CI 工具死于误报，不死于漏报。

2026-07-09 的实证探针（120 行一次性代码，5 份真实 CLAUDE.md/AGENTS.md/SKILL.md，约 230 个 token）验证了以上判断：绑定率 60–90%，零误报（一个边界案例自己指出了修复方式），每个覆盖缺口都对应一个可写的机械 resolver 而非 NLU 需求。数据与教训见 [03-probe-2026-07-09.md](03-probe-2026-07-09.md)。

## 4. 竞争地图

### 4.1 直接竞品

**[agents-lint](https://github.com/giacomo/agents-lint)** 是最近的对手，口号几乎相同（"Your AGENTS.md is probably lying"）。现状（2026-03 v0.5.0）：TypeScript、10 stars、20 commits；检查路径存在性、npm scripts、依赖、框架时代特征（React/Laravel 等 pattern）、结构 lint；有 `--format json` 和交互式 `--fix`；**仅支持 npm 生态**。

它的存在是双重信号：楔子被验证（别人独立看到了同一痛点），且窗口正在关闭。差异化不靠功能列表，靠架构代差：

| 维度 | agents-lint | attest |
|------|------------|--------|
| 机制 | 规则清单（路径规则 + npm 规则 + 框架 pattern 各写一套） | 单一绑定引擎：token → 多命名空间 resolve → verdict，加生态 = 加 resolver |
| 生态 | npm only | 多语言（npm/cargo/go/python/make…），Rust 单二进制 |
| 精度纪律 | 含"年份引用""框架时代"等噪声启发式 | 铁律：误差只朝沉默/绿，近失只降级为 suspect，永不误红 |
| 存量采纳 | 无 | 基线棘轮：只对新增 broken 亮红，brownfield 一天上车 |
| 时间性 | 无状态扫描 | claims.lock + 哈希锚点：drift = 曾经绿的绑定变红 |
| 散文断言 | 无 | 银档：作者时点 LLM 抽取，锚点必须确定性绑定才入库 |
| Agent 闭环 | 交互式 fix | 机器可执行报告（attest.report.v1）→ agent 自动修复 → 复跑变绿 |

**Swimm**（企业级、SOC2、$17–28/席/月）走的是作者时点耦合：写文档时用他家编辑器把片段绑到代码行，专利 Auto-sync 靠 git 历史启发式跟随。attest 的立场相反：**零作者负担**——不要求任何人改变写文档的方式，事后从任意 markdown 中绑定。Swimm 面向企业知识库，attest 面向仓库内文档 + CI，两者可长期共存。

### 4.2 相邻工具（互补而非竞争）

- **AgentLint / AgentLinter**（agentlint.app / agentlinter.com）：harness 健康度审计与 LLM 打分（结构/清晰度/安全），不做仓库事实核验——不同的工作。
- **cclint / claudelint**：对 Claude Code 配置文件做 schema 校验（frontmatter、settings）——互补，attest 可吸收为一个 resolver 类别。
- **doctest / mdbook test / pytest-codeblocks**：只执行代码块，不管指涉；执行式核验是 attest 阶梯的最高档（默认关闭）。
- **lychee**：URL 检查，成熟，集成而非重造。
- **oasdiff / spec-drift 类**：API spec 对实现的 drift，不同构件；attest 的路由 resolver 未来可覆盖一部分。

### 4.3 差异化一句话

> agents-lint 是一组规则，attest 是一个判定引擎。规则清单的天花板是作者想到的检查；绑定引擎的天花板是 resolver 库的积累——而积累正是护城河所在。

## 5. 护城河逻辑

对标 spoor 的五条护城河判据：

1. **必经边界**：每次 CI、每个 agent session 都要面对"文档还对吗"。✅
2. **正确性客观可测**：一个绑定要么成立要么不成立；golden corpus 可回归。✅
3. **长尾积累**：resolver 库（每个生态的 import 语法、路由框架、配置格式、工具子命令表）与挖矿语料（git 历史中真实 drift 案例）都随时间复利，抄得走代码抄不走语料。✅
4. **小 API 面**：一个动词 `check`（外加 `baseline`，后期 `extract`）。✅
5. **本地、确定、便宜**：CI 内零 LLM 调用，毫秒级，可复现。✅

探针已经演示了积累的形态：一次运行暴露的每个失误（Go import 被误当路径、YAML 键无处绑定、相对基目录错误）都精确映射到"一个 20 行的确定性 resolver"，而不是"提升模型理解力"。三年后这个 resolver 库就是新的 ZIP 炸弹防御——spoor 式的、时间换来的资产。

## 6. 命名与发布名

事实核查（2026-07-09）：

- crates.io：`attest` **已占用**（"Dead simple test framework for the age of AI"，2026-06 更新，活跃）；`attest-core` 可用；`aver`、`adduce` 均已占用。
- npm：裸名 `attest` 被 9 年前的 a11y 库占用。
- 心智冲突：GitHub Artifact Attestations（`actions/attest`、`@actions/attest`）占据了"attest"在供应链安全语境的 SEO。

决定：**项目名与二进制名保持 `attest`**（词义精准：出庭作证——文档的每句话都要能作证；与 spoor/sift/vouch/glyph 的单音节动词谱系一致）。发布名走 spoor 先例：

| 渠道 | 名称 |
|------|------|
| crate | `attest-core` / `attest-cli`（发布前确认后者可用） |
| 二进制 | `attest` |
| npm | `@harrisonwang/attest`（npx 零安装入口） |
| GitHub Action | `harrisonwang/attest-action` |
| brew/scoop | 复用 spoor 的 tap/bucket |

SEO 与 GitHub 特性重名是已知代价；搜索词打 "attest docs" / "文档回归测试"，不与供应链语境正面争。

## 7. 风险清单

| 风险 | 判断与对策 |
|------|-----------|
| harness 厂商内置（Claude Code 自带 CLAUDE.md 体检） | 楔子可能被收编，引擎收编不了：多生态 resolver、通用文档、lock/棘轮、修复闭环都超出 harness 职责边界。楔子丢了换滩头（README、docs 站点），资产还在 |
| agents-lint 先发滚雪球 | 它 npm-only、规则式，重写成绑定引擎等于重做。窗口以月计不以年计——P0/P1 必须快（见 roadmap） |
| 误报毁声誉 | 铁律写进设计公理（01-design §1）；对外发的修复 PR 只用金档确认项；基线棘轮保证老仓库首日不炸 |
| 覆盖率不及预期（文档大量指涉绑不上） | P0 的挖矿实验就是为此设的 kill criteria：金档覆盖历史 drift 案例 <60% 则重新设计（见 roadmap §P0） |
| 名字撞车 | 上文 §6，接受并绕行 |

## 8. 本方案文档地图

- [00-vision.md](00-vision.md) — 本文：定位、竞争、护城河、命名、风险
- [01-design.md](01-design.md) — 核心设计：绑定模型、resolver 规格、verdict、报告协议、lock、架构
- [02-roadmap.md](02-roadmap.md) — 阶段路线：P0–P4、kill criteria、发布策略、成功指标
- [03-probe-2026-07-09.md](03-probe-2026-07-09.md) — 立项实证：探针方法、数据、五个教训到设计决策的映射
