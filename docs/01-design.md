# attest — 核心设计

> 状态：立项方案 · 2026-07-09
> 前置阅读：[00-vision.md](00-vision.md)（定位）、[03-probe-2026-07-09.md](03-probe-2026-07-09.md)（实证依据）

## 1. 设计公理

三条公理决定所有后续取舍，冲突时以序号小者为准：

1. **误差只朝沉默，永不朝误红。** 每个 resolver 的不确定都必须落向"不报告"或"降级为 suspect"，绝不落向"报 broken"。CI 工具死于误报。
2. **分类靠绑定，不靠理解。** 不解析句义。token 是什么，由它能绑定到仓库的哪个命名空间决定；仓库是判定器。
3. **NLU 不进 CI。** LLM 只允许出现在作者时点（抽取散文断言，产物入库走 code review），CI 内的每次运行都是纯确定性、可复现、零网络。

## 2. 概念模型

三个名词贯穿全部设计：

```
文档 (markdown)
   │  提取 extract        —— CommonMark 解析,零歧义
   ▼
Token  (带 doc 路径 + 行号 + 所在行原文)
   │  绑定 bind           —— 挨个命名空间尝试,仓库为 oracle
   ▼
Binding (token × 命名空间 × 指称对象 × tier × 内容哈希)
   │  裁决 verdict        —— 绑定状态 + 语境守卫 + 基线比对
   ▼
Finding (verified / broken / suspect / silent)
```

**Drift 的定义**：一个曾经成立（或显然应当成立）的绑定不再成立。它是存在性/一致性判断，不是语义判断。

## 3. 提取规范（extract）

解析器：CommonMark（Rust 用 `pulldown-cmark`）。只从两类节点取 token：

| 来源 | 处理 |
|------|------|
| inline code span | 整个 span 为一个 token。若形如命令行（首词是已知工具或含空格+flag 形态），进入命令行解析（见下） |
| fenced code block，且语言标注 ∈ {bash, sh, shell, zsh, console} | 逐行解析为命令行 token。**无语言标注的 fence 一律忽略**（探针教训 #3：ASCII 大纲、目录树会混入） |

其他一切——正文散文、标题、无标注 fence、非 shell 语言的 fence——v1 不产 token（散文断言走 §10 银档）。

命令行解析规则：

- 去掉 `$ ` 提示符前缀、行内注释、续行符拼接；heredoc 整段跳过。
- 环境前缀 `FOO=bar cmd ...` 剥离赋值后取 `cmd`。
- 占位符 `<...>`、`{...}`、`$VAR`、glob `*` 处为通配，绑定时按通配匹配，匹配不到不报错（公理 1）。
- 每行产出：首词（命令 token）+ 结构化参数（`run <script>`、`--filter <pkg>` 等由工具表驱动，见 §11）。
- 行内多词 span（`cargo test`、`git commit -m`）与 fenced 行走**同一个**命令行解析器（探针教训 #1：v0 只对 fenced 做了解析，低估绑定率 30%+）。

每个 token 携带溯源：`(doc 相对路径, 行号, 列区间, 所在行原文)`。所在行原文用于语境守卫（§7）与报告展示。

## 4. 架构与 crate 布局

镜像 vouch/spoor 的分层，但 I/O 边界因问题形状不同而调整：vouch 的输入（IR + diff）小到可以整体传入，attest 的判定器是整个仓库，全量预载不现实。因此 core 的纯度定义为 **I/O-behind-trait**：core 不直接碰文件系统/进程/网络，一切事实通过 `RepoFacts` trait 查询；给定同一个 facts 实现，输出完全确定。

```
attest/
├── crates/
│   ├── attest-core/          # 纯逻辑:extract + bind + verdict + report 组装
│   │   ├── extract.rs        # CommonMark → Token[]
│   │   ├── token.rs          # Token / 命令行解析 / 占位符
│   │   ├── facts.rs          # trait RepoFacts(见下)
│   │   ├── resolve/          # 每个 resolver 一个文件,注册表模式
│   │   │   ├── path.rs  script.rs  pkg.rs  cmd.rs  env.rs  symbol.rs  ...
│   │   ├── verdict.rs        # 裁决 + 语境守卫 + 基线比对
│   │   └── report.rs         # attest.report.v1 组装
│   └── attest-cli/           # 适配层:facts 采集(fs/git/rg) + 渲染(tty/json/github) + baseline 存取
└── docs/
```

```rust
/// core 与世界的唯一边界。CLI 给真实实现,测试给 FakeRepoFacts。
pub trait RepoFacts {
    fn file_exists(&self, base: Base, rel: &str) -> bool;
    fn find_basename(&self, name: &str) -> Vec<String>;          // path~ 近失用
    fn script(&self, name: &str) -> Option<ScriptOrigin>;         // package.json/Make/just/cargo alias
    fn workspace_pkg(&self, name: &str) -> bool;
    fn binary_known(&self, name: &str) -> BinKnowledge;           // repo 自产 bin / PATH / 工具表
    fn grep_word(&self, word: &str) -> Option<FirstHit>;          // 符号/env/字面量,懒执行
    fn config_key(&self, file_hint: Option<&str>, key: &str) -> Option<FirstHit>;
    fn content_hash(&self, base: Base, rel: &str) -> Option<u64>; // lock 锚点用
}
```

采集策略：文件树来自 `git ls-files`（自动尊重 .gitignore；非 git 目录退化为受限 walk），scripts/包名/target 表启动时一次性建，grep 类懒执行带 memo。全部只读。

## 5. Resolver 协议与 v1 清单

```rust
pub enum BindOutcome {
    Bound { ns: Namespace, referent: String, tier: Tier },  // Tier: Exact | Normalized | Relocated
    NearMiss { suggestion: String, note: String },           // → suspect,永不 broken
    NoMatch,                                                 // → 该 resolver 沉默
}
```

绑定顺序即命名空间优先级，首个 `Bound` 胜出；所有 resolver 的 `NearMiss` 汇总保留（供报告给建议）。一个 token 全部 `NoMatch` → verdict `silent`，默认不出现在报告里。

v1 resolver 清单（按优先级）：

| # | resolver | 绑定 | 近失（suspect） | 沉默条件 |
|---|----------|------|----------------|---------|
| 1 | path | 按 §6 基目录序存在 | basename 在别处存在（探针 `path~` 档）→ 给出新位置 | 不含 `/` 且无已知扩展名 |
| 2 | script | package.json scripts / Makefile / justfile target / cargo alias | 编辑距离 ≤2 的同源 script（`test`→`test:unit`） | — |
| 3 | pkg | workspace 包名（pnpm/npm/cargo members） | 前缀匹配 | — |
| 4 | cmd | repo 自产 bin（Cargo bins、package.json bin）/ PATH / 工具表（§11） | 工具表内已知改名 | 单个普通英文词 |
| 5 | go-import | go.mod module + require 表 + vendored stdlib 表 | — | 无 go.mod 时整体禁用（探针教训 #4） |
| 6 | env | `[A-Z][A-Z0-9_]{2,}` 且 grep_word 命中源码 | — | grep 不中（可能是文档自造名） |
| 7 | config-key | 同文档/邻近提到的 YAML/TOML/JSON 里的键 | — | 无候选文件（探针教训 #5：`ask_audience`） |
| 8 | symbol | 标识符形态且 grep_word 词边界命中源码 | — | 长度 ≤2 或纯常用词 |

symbol 用 grep 而非 tree-sitter 是刻意的 v1 取舍：grep 的假绿（词出现在注释里）落向漏报，公理 1 允许；tree-sitter 精确索引进 v2，只收紧不翻案。每个 resolver 20–80 行、独立 golden 测试、注册表注册——**加生态 = 加 resolver 文件**，这就是护城河的积累单元。

## 6. 作用域规则（monorepo）

token 的解析基目录（`Base`）按序尝试，首个命中即绑定：

1. **doc-dir**：文档自身所在目录（探针教训 #2：例如某个被扫仓库 SKILL.md 里的 `phases/`，只有相对文档自身才成立）
2. **project-root**：从文档向上最近的含 manifest（package.json/Cargo.toml/go.mod/pyproject.toml）的目录
3. **repo-root**：仓库根
4. **workspace 命名空间**：与目录无关的名字空间（包名、workspace member、`--filter` 目标）

多基目录同时命中且指向不同对象时：取序号最小者绑定，其余记入 finding 的 `evidence`（不报警，供人排歧）。`attest.toml` 可对 doc glob 钉死 scope（§13），默认零配置；多个 scope glob 同时命中时，字符串更长的具体规则优先，同长度按字典序稳定裁决。

## 7. 裁决（verdict）与语境守卫

| verdict | 含义 | 默认 CI 行为 |
|---------|------|-------------|
| `verified` | 绑定成立 | 静默计数 |
| `broken` | 该 resolver 判定"理应绑定而绑不上"（如路径形态完备但不存在、`pnpm run x` 而 x 不在任何 scripts） | **失败**（仅此一档，且过基线棘轮后） |
| `suspect` | 近失 / 语境守卫降级 / 多基歧义 / 银档未确认 | 警告，不失败 |
| `silent` | 所有 resolver NoMatch | 不出现（`--verbose` 可见） |

**语境守卫**（探针教训 #6，jadeenvoy 的 `CONVENTIONS.md`——文档原文"未配置；可建 symlink"，指涉物不存在是文档本身声明的事实）：finding 携带所在行原文，若行匹配否定/假设模式（`未配置|不存在|可选|已废弃|如果|若|optional|deprecated|not yet|TODO|e.g.|例如`），`broken` 自动降为 `suspect`。廉价、诚实、可关闭；语境的严格判定属于银档。

## 8. 基线棘轮（brownfield 采纳）

存量仓库首次运行必然翻出历史欠账，如果首日就全红，没人会装。棘轮模式：

```bash
attest check              # 报告全部 findings,exit code 按当前 broken 数
attest baseline update    # 把当前 broken 记入 .attest/baseline.json(入库)
attest check              # 此后只对 baseline 之外的**新增** broken 失败
```

baseline 条目按 `(doc, token, ns)` 记，不按行号（文档挪动不应击穿棘轮）。修一条少一条，`attest baseline update` 收紧。这是 eslint-baseline / ratcheting linter 的成熟模式。

## 9. 增量模式

```bash
attest check --since origin/main
```

由 `git diff --name-only` 驱动：变更文件 ∈ 文档集 → 全量重查该文档；变更文件 ∈ 某绑定的指称对象（或其 manifest）→ 重查所有指向它的绑定。复杂度 O(变更) 而非 O(仓库)。这也是未来 vouch 集成点：IR 声明"改了 CLI flag X"→ 直接定向到引用 X 的绑定，不用 diff 推断。

## 10. claims.lock 与银档（v2，散文断言）

金档覆盖不到反引号外的散文指涉（"API 服务在 apps/api 下"、"需要 Node 18"）。扩展路径分三级，全部不违反公理 3：

1. **形状候选 + 绑定确认**（纯机械）：正文中路径形态、版本形态、ALL_CAPS 形态的裸词，按形状提候选、按绑定定生死。随机英文词不会碰巧存在于文件树——oracle 保证精度。
2. **strict-lint 反向硬化**：`attest check --strict` 提示"这个词长得像路径但没加反引号/绑不上"。gofmt 和 eslint 出现之前，Go 和 JS 也没有强规范——**工具创造规范**。lock 文件就是事实规范。
3. **银档：作者时点 LLM 抽取 + 锚定入库**：

```yaml
# .attest/claims.lock —— LLM 只在生成此文件时出现,CI 只读它
schema: attest.claims.v1
claims:
  - claim: "前端是 SvelteKit,静态 adapter,构建产物在 build/"
    doc: CLAUDE.md:12
    status: approved            # proposed → (git review) → approved
    anchors:
      - { ns: path,   ref: "apps/web/svelte.config.js", hash: "8f3a2c1e" }
      - { ns: symbol, ref: "adapter-static",             hash: "d41d8cd9" }
```

规则与 answer-trace 银档同源：**LLM 提议的每个锚点必须当场确定性绑定成功，否则条目不生成**（不洗白幻觉）。审批流就是 git review——lock 是入库文件，`proposed` 条目在 PR 里被人/agent 审阅后合入即 `approved`，不造新 UI。lock 使用严格 schema：未知字段、空 claim/doc/anchors/ref、非法哈希或零行号都直接作为输入错误退出，避免拼写错误让审批静默失效。CI 逐条复查来源文档与锚点：来源文档或锚点删除 → `broken`；存在 + 哈希未变 → 断言自动仍成立（零成本绿）；哈希变了 → `suspect("锚点代码已变更,断言待复核")`，由人或 agent 复核后更新 lock。**哈希变更永不直接判 broken**（公理 1：代码变了 ≠ 断言错了）。

## 11. 第三方工具表

`git commit`、`pnpm install`、`cargo test` 这类外部工具调用，绑定依据是 vendored 的子命令/flag 表（v1 覆盖 git、cargo、pnpm、npm、yarn、uv、go、make、docker、gh、wrangler，先不分版本）。表是数据文件不是代码，社区可 PR——这是第二类语料资产。表外工具 → cmd resolver 按 PATH 判存在，flag 不校验（沉默）。

**执行安全阶梯**（硬立场，写进 README）：

| 档 | 行为 | 默认 |
|----|------|------|
| static | 只读文件系统 + 工具表,不执行任何东西 | ✅ 唯一默认 |
| introspective | `--allow-exec=repo-bins`:允许运行**仓库自产**二进制的 `--help` 并解析 | 显式 opt-in |
| executive | 真执行文档中的命令(沙箱) | v1/v2 不做,可能永不做 |

attest 扫描不可信仓库的文档时不能成为代码执行入口——这与 spoor 的 ZIP 炸弹防御是同一种产品性格。

## 12. 报告协议 attest.report.v1

面向两个读者设计：人（TTY 渲染、GitHub PR annotations）与 agent（JSON，字段自足，修复所需信息全部就地给出——answer-trace 的血统）。

```json
{
  "schema": "attest.report.v1",
  "root": ".",
  "commit": "6b98796",
  "stats": { "docs": 3, "tokens": 187, "verified": 141, "broken": 2, "suspect": 5, "silent": 39 },
  "findings": [
    {
      "id": "f1",
      "verdict": "broken",
      "token": "pnpm run test:e2e",
      "doc": "CLAUDE.md",
      "line": 42,
      "context": "提交前运行 `pnpm run test:e2e` 确认端到端通过",
      "ns": "script",
      "evidence": { "searched": ["package.json#scripts", "apps/*/package.json#scripts"], "nearest": "test:e2e-ci" },
      "suggestion": "脚本已改名,文档应改为 `pnpm run test:e2e-ci`",
      "baseline": false
    }
  ]
}
```

`suggestion` 只在近失置信度高时给出，措辞永远是"文档应改为"而非"代码有错"——attest 裁决的是文档对仓库的忠实度，不裁决仓库本身。

**修复闭环**（检测是产品，修复是 demo）：配套 Claude Code skill `/attest-fix`——读 report JSON → 逐条改文档 → 复跑 `attest check` → 绿了才收工。棘轮 + 机器可执行报告 + agent，三件事拼出"文档自维护"，这是 Swimm 一代做不到的演示。

## 13. 配置 attest.toml（全部可省略）

```toml
[docs]
include = ["CLAUDE.md", "AGENTS.md", ".claude/**/*.md"]   # 默认值;README.md 等自行加入
exclude = ["**/node_modules/**"]

[resolvers]
symbol = true          # 每个 resolver 可单独关

[policy]
fail-on = "broken"     # broken | never
context-guard = true

[scope]
"docs/ops/**" = "repo-root"   # 钉死某类文档的解析基目录
```

## 14. 测试与语料策略

- **golden 测试**：core 全部走 `FakeRepoFacts`——(文档文本 + 假事实) → 期望 findings，纯内存、毫秒级、无环境依赖。每个 resolver 的每条规格（绑定/近失/沉默）至少一条 golden。
- **挖矿语料**：git 历史里"commit A 改了代码、commit B 修了文档"的配对就是带标注的真实 drift 案例。P0 的挖矿脚本从自有仓库 + 知名开源仓库攒 200+ 例，同时回答三个问题：断言类型的真实频率分布（决定 resolver 优先级）、精度基线、launch 素材。
- **self-host**：attest 的 CI 用 attest 检查自己的 docs/ 与 CLAUDE.md，第一天起 dogfood。
- **精度指标**：金档误报率目标 **0**（对全语料）；每次发布前对语料全量回归。

## 15. 遗留的两个开放问题（已收敛）

立项讨论中挂起的两个问题，方案如下，实现中再验证：

1. **lock 审批流**：复用 git review（§10）。`proposed` 条目由 `attest extract` 产生，PR 审阅即审批，无新 UI。agent 可以代人审（Claude Code 里 review lock diff），但合入动作留给人的 merge 权限——与 vouch 的"公证人不替你签字"立场一致。
2. **monorepo 作用域**：四级基目录序 + 钉 scope 配置（§6）。探针里的 `phases/` 示例已验证 doc-dir 优先的必要性；歧义不报警只记 evidence，等真实语料里出现歧义伤害再考虑升级策略。
