# attest — 核心设计

> 状态：立项方案 · 2026-07-09
> 前置阅读：[00-vision.md](00-vision.md)（为什么做）、[03-probe-2026-07-09.md](03-probe-2026-07-09.md)（摸底数据）

## 三条铁律

attest 所有的设计取舍都由下面三条决定。有冲突的时候排前面的说了算。

第一条：拿不准就不说话，绝不能瞎报。每个检查模块的不确定结果要么不报，要么只给个"提醒"（suspect），永远不会报成"错误"（broken）。CI 工具死于误报，这是血的教训。

第二条：分类靠配对，不靠理解。不分析句子的意思。一个 token 是什么，取决于它能在仓库里对应上什么东西。仓库本身就是裁判。

第三条：AI 不进 CI。LLM 只能出现在作者写文档的时候（帮忙从正文里抽断言，产物入库走 code review）。CI 里每次运行都是纯确定性、可复现、零网络的。

## 整个流程怎么走

四个步骤，贯穿全部设计：

文档（Markdown）经过 CommonMark 解析，抽出 token。每个 token 带上它在哪个文件、第几行、第几列、以及它出现的那一行原文。接着挨个检查模块去试着配对——这个 token 能在文件树里找到吗？在 package.json 的脚本里吗？是某个包的包名吗？哪个模块先对上就归哪个。对上之后还要过语境判断："作者这句话是在说'不要这样做'还是在举例？这条路径会不会是运行时产物？"确定无疑是对不上才报 broken。最后出来的结果只有四种：verified 对上了，broken 确定对不上，suspect 拿不准提醒一下，silent 没结果不出声。

所谓的 drift，说的就是以前能对上、现在对不上了。跟作者想表达什么没关系，跟仓库里实际有什么有关系。

## 怎么从文档里抽信息

解析器是 CommonMark（Rust 这边用 pulldown-cmark）。attest 只从三类东西里取 token。

反引号里的内容（inline code span）：整个反引号包住的文字就是一个 token。如果它看起来像命令行——比如包含空格和参数，或者首词是已知的工具名——就继续按命令行做结构化解析。

用 bash、sh、shell、zsh、console 标注的围栏代码块：里面的每一行都当成命令解析。注意，没标注语言的代码块全部跳过。摸底的教训是没标注的代码块经常是画 ASCII 目录树、列大纲、贴日志，拿来当命令解析会出一堆噪音。

链接和图片的目标（`[文字](docs/arch.md)` 里括号那部分）：带 scheme 的（http、mailto 这些）是外部世界，`#` 开头的是页内跳转，`/` 和 `~` 开头的不在仓库相对语义里，这三种全部跳过。剩下的去掉锚点和查询串，就是对本仓库文件的引用。链接目标在语法上只可能是文件，所以这类 token 只走路径检查，不会被脚本、命令这些角度错绑。

正文散文、标题、没语言标注的围栏块、非 shell 语言的围栏块——v1 一概不碰。散文里的断言是 claims.lock 的事，后面会讲。

命令行的解析规则不复杂。`$` 开头的提示符和行内注释去掉，用反斜杠续的行拼起来，heredoc 整段跳过。`FOO=bar command ...` 这种带环境变量前缀的，把赋值剥了取后面的命令名。`<...>`、`{...}`、`$VAR`、`*` 这类占位符和通配符，匹配不到就不报错（铁律第一条）。最终每行出来一个命令 token 和它的参数列表。

值得提一句，反引号里的多词内容（像 `cargo test`、`git commit -m`）和围栏块里的命令行走的是同一套解析器。摸底第一版只解析了围栏块，绑定率低估了 30% 以上。

## 代码怎么组织的

attest 分两层。attest-core 是纯逻辑，不碰文件系统、不碰进程、不碰网络，所有外部信息都通过一个 trait `RepoFacts` 来问。你给它同样的 facts 实现，输出完全确定。attest-cli 是适配层，负责真实的文件扫描、manifest 解析、Git 信息采集这些脏活。

```
attest/
├── crates/
│   ├── attest-core/          # 纯逻辑：抽取、配对、判断、降级
│   │   ├── extract.rs        # CommonMark → Token[]
│   │   ├── model.rs          # 数据类型
│   │   ├── facts.rs          # RepoFacts trait 定义
│   │   ├── glob.rs           # 通配匹配
│   │   ├── guard.rs          # 四类降级守卫
│   │   ├── resolve/          # 八个检查模块，一个模块对应一类东西
│   │   ├── skill.rs          # SKILL.md frontmatter 校验（文档级）
│   │   └── engine.rs         # 流程编排 + 基线比对 + lock 复查
│   └── attest-cli/           # 适配层
│       ├── main.rs           # 命令行入口
│       ├── check.rs          # 检查流程（全量 / --since / --strict）
│       ├── facts.rs          # 事实采集
│       └── ...               # extract / llm / vouch / store / render 等
```

```rust
pub trait RepoFacts {
    fn path_bases(&self, doc: &str) -> Vec<Base>;         // 路径解析的查找起点
    fn resolve_path(...) -> Option<String>;                // 这个路径存在吗
    fn glob_paths(...) -> Vec<String>;                     // 通配匹配
    fn find_basename(&self, name: &str) -> Vec<String>;    // 文件名搬家推测
    fn path_ignored(&self, rel: &str) -> bool;             // 命中 .gitignore？
    fn script(&self, name: &str) -> Option<ScriptOrigin>;  // package.json 等
    fn workspace_pkg(&self, name: &str) -> bool;           // workspace 包名
    fn binary_known(&self, name: &str) -> BinKnowledge;    // 命令是否存在
    fn grep_word(&self, word: &str) -> Option<FirstHit>;   // 源码中搜索
    fn config_key(...) -> Option<FirstHit>;                // 配置文件中的 key
    fn content_hash(...) -> Option<String>;                // 文件内容指纹
}
```

事实采集的策略是：文件树用 `git ls-files`，自动尊重 .gitignore，非 git 目录退化成受限的目录遍历。脚本名、包名、二进制命令表启动时一次性建好。词搜索在第一次调用时对可搜索文件建好索引，之后全查表，不再逐词扫全仓库。`.gitignore` 的检查走 `git check-ignore` 并缓存。全部只读，不写任何东西。

## 检查模块怎么工作

每个检查模块对应一类东西（路径、脚本、包名、命令、Go 的 import、环境变量、配置项、代码符号）。每个模块对同一个 token 给出四种可能的结果之一。

Bound 表示配对成功，找到了。NearMiss 表示差一点就对上（比如脚本改了个名），给个 suspect 提醒但不报错。Broken 表示这个模块判断"理应能对上但就是没有"——比如路径格式完整但文件不存在。Ignored 是占位符占位符之类直接跳过。NoMatch 表示这个模块不归它管，沉默。

检查有优先级。排在前面的是路径和脚本——这两类最常见，命中率最高。排后面的是环境变量、配置项、代码符号——这些东西需要搜索，成本更高而且更容易误匹配。第一个 Bound 的结果直接胜出，后面的不看了。所有模块的 NearMiss 会汇总起来，在报告里给建议。

改法建议是一件消耗信任的事情。只有一个明确候选的时候才给——"文档的 `test` 应该改成 `test:unit`"这种。有多个可能性的时候只列出来，不替用户做决定。

八个检查模块的简要说明：

路径检查：判断 token 像不像文件路径（有斜杠、有已知扩展名），按文档所在目录、项目根、仓库根的优先级顺序依次找。支持 glob 通配。文件被挪到别处了会提醒，唯一命中时才给修改建议。某些形状不做猜测——比如不含斜杠也没有已知扩展名、父目录不存在、`owner/repo` 这种仓库缩写。Go 项目里如果没 go.mod，Go import 那类路径也不往路径模块送。SKILL.md 里另有一条收紧：根级裸文件名（`llms.txt` 这种没带目录的）在仓库里毫无踪迹时只提醒不报错——skill 教的是 agent 去目标仓库干活，这类名字多半说的是用户仓库该有的文件或跑完才生成的产物；带目录的路径不受影响，skill 自带的 references/ 附件缺了照样红。

脚本检查：能解析 `npm run`、`pnpm run`、`make`、`just`、`cargo` alias 这些模式。目标名如果在 package.json、Makefile、justfile、.cargo/config.toml 里能找到就算配对。差一两个字（编辑距离 ≤2）会给 NearMiss 的提醒。

包名检查：检查 workspace 里的包名。支持 `pnpm --filter @scope/pkg`、`npm -w pkg`、Cargo workspace member 这些模式。

命令检查：检查二进制命令是否已知。来源可能是仓库自己编译的（Cargo bins、package.json bin）、系统 PATH 上的、或者内置工具表里的。工具表覆盖了 git、cargo、pnpm、npm、yarn、uv、go、make、docker、gh、wrangler 这些常用工具的子命令。表是数据文件不是代码，社区可以 PR 扩充。表里没有的工具走 PATH 检查，只验证命令存在，不校验参数。

Go import 检查：有 go.mod 的仓库，在 module 路径和 require 表里查 Go 的 import 路径。内置了一份标准库的包名表。没有 go.mod 的话这个模块整体关掉——摸底的教训是没 go.mod 的仓库里，Go 风格的路径被当成普通路径检查，出了很多噪音。

环境变量检查：全大写下划线格式的 token（至少两个字符），去源码里搜这个词。搜不到就算了，可能是文档自己造的名字。

配置项检查：token 如果是配置文件的键名格式，去同文档里提到的或者就近的配置文件里搜。

符号检查：标识符格式的 token，去源码里搜。长度太短或者纯常用词的不搜——降低误匹配。

每个检查模块 20 到 80 行代码，有独立的 golden 测试，通过注册表组装。加一个新生态就是加一个模块文件——这就是积累的单元。

token 检查之外还有一个文档级的校验：SKILL.md 的 frontmatter。skill 靠 name 和 description 注册进 agent 的技能表，这两个字段坏了 skill 不报错，只是安静地消失，对 agent 的伤害和死路径同类。frontmatter 块缺失、YAML 解析失败、必填字段不在或为空算 broken；形状不合规范（大写、超长）只提 suspect，因为各家宿主的执行松紧不一。name 还是 `your-skill-name` 这种占位符的按模板处理，整个不出声；references 和 templates 目录下的 SKILL.md 同理跳过。

## 多级目录怎么找文件

token 里出现了一个路径，从哪个起点开始找？按顺序试：

先是文档自己所在的目录。摸底的教训——某个仓库的 SKILL.md 里写了 `phases/`，只有相对这个 SKILL.md 的位置才能找到。再是项目根目录，从文档位置往上找，碰到第一个带 manifest（package.json、Cargo.toml、go.mod、pyproject.toml）的目录。最后才是仓库根目录。

如果多个起点都找到了匹配，但指向不同的文件，取优先级最高的，其他的记在报告里供人判断，不报警。

不需要写配置，默认就能用。如果有特殊需求，可以在 `attest.toml` 里钉死某类文档的解析起点。多条 scope 规则同时命中时字符串更长的优先。

## 怎么判断对错

结果的四种等级很简单。verified 是对上了，正常，不出声。broken 是确定对不上——比如路径形状完整但文件不存在，或者 `pnpm run x` 但 x 不在任何脚本里，这种 CI 会失败。suspect 是拿不准——差一点对上的、被守卫降级的、多基目录歧义的、claims.lock 里还没审批的，都归这档，只提醒不失败。silent 是所有检查模块都没产出，不出现在报告里，除非你开了 `--verbose`。

broken 在最终报出之前要过四道守卫。任何一道命中就降成 suspect，理由写在报告里。

结构守卫看 token 是不是在标题行或者表格行里。那是排版需要，不是说文件一定存在。

语境守卫看 token 所在的那一行在说什么。如果句子在说"不要这样做"、"暂未实现"、"比如这样"、"创建文件"、"删除目录"、"分支名以此开头"、"上游的文件已并入别处"——那这条不一致是文档自己预期的，attest 不报错。词表故意收得很小，只有五组高置信词。匹配之前先把反引号里的内容遮掉，避免 token 自身的文字干扰判断。这个设计来自摸底的第六个教训——某份 CONVENTIONS.md 里写着"未配置；可建 symlink"，文件不存在是文档自己说明的。

文档类别守卫对付一种特殊情况：SKILL.md 里的脚本和包名，references 和 templates 目录下的路径，往往是在说别的仓库，不是本仓库该有的。attest 识别出这些场景就不按 broken 报。

形状守卫对付 token 自己长得不像真实路径的情况。`YYYY-MM-DD-notes.md` 是日期模板，`path/to/file` 是占位符，`ComponentName.tsx`、`usePageName.ts` 这种驼峰 Name 收尾的是命名规范里的元变量，`node_modules`、`target`、`dist` 是运行时产物，ASCII 的 `...` 和中文省略号一样按占位处理。仓库自己的 .gitignore 规则也会被检查——文档引用了被 ignore 的路径，多半在说构建产物，有别于真的路径消失。

三条纪律：守卫只看 token 所在的那一行，不看多行——窗口匹配会让前文一个"when"把后面不相干 token 的红也拉下水。想加词先问问自己：这是普遍的语言现象还是只对某一个仓库成立的巧合？巧合只能进语料文件做回归，不能进代码。守卫要廉价、可解释、可关闭，不要变成黑盒。

## 老项目怎么接进来

存量仓库第一次跑 attest 肯定哗哗报一堆。让人先把存量修完再接工具是不现实的。

attest 的办法很简单：先跑一次 `attest baseline update`，把所有当前的 broken 记进 `.attest/baseline.json` 入库。之后再跑 `attest check`，只对基线之外的、新增的 broken 才亮红。修一条、基线少一条、再跑 `attest baseline update` 收紧。这是 eslint-baseline 和 ratcheting linter 验证过的成熟模式。

基线按文档名、token 文本、检查类型来记，不记行号——文档里加两行不该让基线失效。

## 只查改过的

```bash
attest check --since origin/main
```

Git 的 diff 告诉你哪些文件变了。被改的文件如果是文档，就全量重查。被改的文件如果是某个绑定的指向目标（比如脚本的 manifest），就把所有指向它的绑定也重新查一遍。复杂度只跟变更量挂钩，跟仓库大小无关。

这条路往后还能跟 vouch 打通——vouch 的 commit IR 已经声明了"这次改了什么"，attest 直接定向复查相关的绑定，不用靠 diff 反推。

## 正文里的东西怎么处理

前面的机制只处理反引号和围栏块里的 token。但文档正文里经常有这样的句子："API 服务在 apps/api 下"、"需要 Node 18"。这些没打反引号的引用，attest 怎么处理？分三档，全部不违反第三条铁律（LLM 不进 CI）。

第一档是纯机械的。正文里长得像路径的词、像版本号的片段、全大写的名字，按形状捞出来当候选，然后走同样的绑定检查。能在文件树里找到才算数，找不到就算了。随机英文单词不会碰巧挂在文件树里——仓库本身就是裁判。

第二档是 strict 模式。`attest check --strict` 会把"这个普通词长得像路径但没打反引号，也绑不上"标出来。gofmt 和 eslint 出来之前，Go 和 JS 也没有强格式规范——工具可以创造规范。`claims.lock` 就是这个规范的事实载体。

第三档是银档——作者时点用 LLM 帮忙找，但所有锚点都必须当场绑定成功才写入。lock 的例子长这样：

```yaml
# .attest/claims.lock —— LLM 只在生成这个文件时出现，CI 只读它
schema: attest.claims.v1
claims:
  - claim: "前端是 SvelteKit，静态 adapter，构建产物在 build/"
    doc: CLAUDE.md:12
    status: approved            # proposed → (git review) → approved
    anchors:
      - { ns: path,   ref: "apps/web/svelte.config.js", hash: "8f3a2c1e" }
      - { ns: symbol, ref: "adapter-static",             hash: "d41d8cd9" }
```

规则很明确：LLM 提的每一个锚点都必须当场通过确定性绑定，有一个对不上整条 claim 就不要——attest 不替 LLM 洗白幻觉。审批走 git review 就行，lock 是入库的文件，proposed 条目在 PR 里审过了合进去就是 approved，不需要造一套新界面。

CI 里逐条复查：来源文档或者锚点删了就是 broken。还在、哈希没变、断言自动成立，不用人管。哈希变了就是 suspect（"锚点对应的代码改了，断言需要复核"），人或 agent 复核后更新 lock 就行。哈希变化绝对不直接判 broken——代码变了不等于断言错了，这是铁律第一条的延伸。

## 命令行工具的知识库

git、pnpm、cargo 这些外部工具的子命令和参数，attest 内置了一份 vendored 的知识表。v1 覆盖了 git、cargo、pnpm、npm、yarn、uv、go、make、docker、gh、wrangler，先不按版本区分。表是数据文件，社区可以 PR 增补，这是第二类语料资产。表外的工具走 PATH 检查命令是否存在，参数不校验——沉默。

关于执行的安全立场：attest 只做静态检查，不执行任何命令。将来可能加一个显式 opt-in 的选项，允许跑仓库自己编译的二进制来获取 `--help` 信息。在沙箱里真执行文档中的命令——v1 和 v2 都不做，可能永远不做。attest 扫描不可信仓库的文档时不能变成代码执行入口，这是产品底线。

## 结果长什么样

报告同时面向两个读者：人看 TTY 渲染和 GitHub PR 批注，agent 看 JSON。

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
      "evidence": {
        "searched": ["package.json#scripts", "apps/*/package.json#scripts"],
        "nearest": "test:e2e-ci"
      },
      "suggestion": "脚本已改名，文档应改为 `pnpm run test:e2e-ci`",
      "baseline": false
    }
  ]
}
```

report 里的字段都是自足的。agent 读这个 JSON 就知道哪份文档、第几行、哪个 token、出了什么问题、该怎么改。不需要再问用户任何事。

`suggestion` 只在把握大（唯一候选）的时候给。措辞永远指向"文档应改为某"，而不是"代码错了"。attest 判决的是文档对仓库的忠实度，不是仓库本身的对错。

修复闭环大概是这样的：Claude Code 里用 `/attest-fix` skill，agent 读了 report JSON，逐条改文档，再跑 `attest check`，绿了才收工。基线 + 机器可读报告 + agent，三样东西拼在一起，文档就能自己维护自己了。

## 配置文件

所有配置都可以不写。需要定制的话，仓库根目录放一个 `attest.toml`：

```toml
[docs]
include = ["CLAUDE.md", "AGENTS.md", ".claude/**/*.md"]   # 默认值，README.md 等可以自己加
exclude = ["**/node_modules/**"]

[resolvers]
symbol = true          # 每个检查模块可以单独开关

[policy]
fail-on = "broken"     # broken | never
context-guard = true

[scope]
"docs/ops/**" = "repo-root"   # 把某类文档的路径解析起点钉在仓库根
```

## 怎么保证自己是对的

测试分了几个层次。

golden 测试覆盖了 core 的全部逻辑。给一组文档文本和一组假事实，期望得到一组确定的 findings。纯内存跑、毫秒级、不需要任何环境。每个检查模块的每条规格（绑定成功、差一点、沉默）都至少有一条 golden。测试替身和真实实现共用同一套 glob 匹配逻辑，文件树同样带祖先目录——避免"测试过了、生产翻车"的语义分叉。

守卫的语料在 `corpus/guard-cases.jsonl` 里，逐条标注了真实句子该得什么裁决、什么理由。守卫每次调整都要对着这份语料做回归。有些句子的诚实结论就是 broken——缺了就是缺了，不能为了好看往守卫里塞例外。

挖矿语料是从 git 历史里刨出来的——"commit A 改了代码，commit B 修了文档"这样的配对就是带天然标注的真实 drift 案例。摸底阶段从自有仓库和知名开源仓库里攒了 200 多例，同时回答三个问题：各种断言类型的真实分布频率（决定检查模块的优先级）、精度的底线在哪、launch 的时候有什么可展示的素材。

检出率用了双线门禁，两个数字各算各的，不互相凑——broken 率（CI 真会拦住的比例，目标 ≥55%）和 broken+suspect 的总检出（目标 ≥80%）。当前实测 broken 58.6%，总检出 85.0%。

attest 自己的 CI 当然也跑 attest 检查自己的文档，从第一天起 dogfood。已知的盲区也要诚实：没标语言的围栏块不产 token（刻意取舍），所以设计文档里用纯围栏块画的目录树，attest 自己看不见。这类漂移只能靠人和 review。

误报率的目标是零。每次发布前对全部语料做全量回归。

## 两个还没完全想好的问题

lock 的审批流程：用 git review 就行。`attest extract` 产出 proposed 条目，PR 里审过合进去就是 approved。不造新 UI。agent 可以帮人审 lock 的 diff，但合入的动作留给人的 merge 权限——跟 vouch 的"公证人不替你签字"一个立场。

monorepo 的路径起点：四级优先级加上 `attest.toml` 里的 scope 配置，目前够用了。摸底的 `phases/` 例子验证了文档自身目录优先的必要性。歧义的时候不报警只记 evidence，等真实语料里出现因为歧义造成的坏结果再考虑怎么升级。
