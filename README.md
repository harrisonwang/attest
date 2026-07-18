# attest

attest 是一个文档一致性检查工具。说白了就是：文档里写了什么，仓库里就得有什么。对不上就报。

你写 CLAUDE.md 或者 AGENTS.md 的时候，告诉 AI 可以去跑 `pnpm run test`，可以读 `src/config.ts`，依赖了 `@scope/core` 这个包。过一阵子你重构了项目，脚本改成了 `pnpm run check`，配置文件挪到了 `packages/config/src/index.ts`，但忘了改文档。attest 就是来抓这种不一致的。

它怎么干活的？从 Markdown 里把反引号包住的词、shell 代码块里的命令、链接指向的本地文件全抽出来，去仓库里一个一个对：这个路径还在不在？这个脚本名 package.json 里有没有？这个命令本地能跑吗？对上了就过，对不上就告诉你哪儿出了问题。SKILL.md 还会多做一层 frontmatter 校验——name 和 description 写坏了，skill 不报错，只是安静地从 agent 的技能表里消失，这也是文档对 agent 撒谎的一种。

关键点是：attest 不靠 AI 理解文档在说啥，也不执行你的代码。它就是机械比对。碰见拿不准的情况就打个标记，绝不瞎报错把 CI 搞红。

## 安装

```bash
cargo install --path crates/attest-cli
```

嫌编译慢可以用 npm 零安装：

```bash
npx @harrisonwang/attest check
```

macOS 或 Linux：

```bash
brew install harrisonwang/tap/attest
```

Windows：

```bash
scoop bucket add harrisonwang https://github.com/harrisonwang/scoop-bucket
scoop install attest
```

## 使用

```bash
# 啥参数都不传，自动扫仓库里的 CLAUDE.md、AGENTS.md、SKILL.md，以及 .claude 下的所有 md
attest check

# 指定文档，换种输出格式
attest check README.md --format json

# GitHub Actions 用这个格式，能在 PR 里直接标出来
attest check --format github

# 文档正文里有些路径没打反引号，但长得像文件？加 --strict 把它们也揪出来（只给提示，不报错）
attest check --strict

# 老项目接进来，先把已有的问题记一笔，以后只拦新问题
attest baseline update
attest check

# 只检查跟 origin/main 相比有变化的文档
attest check --since origin/main

# 搭配 vouch 的 Commit IR，还能把变更影响的文档也顺便查了
attest check --vouch-ir .vouch/commit-ir.json

# 从正文里提取"这里有个文件 / 命令 / 配置"这类断言，写进 claims.lock
attest extract

# 想让 LLM 帮你找更多断言？可以，但它提的东西最终还是得靠算法验证通过才算数
OPENAI_API_KEY=... attest extract --llm
```

退出码的意思：`0` 没新问题，`1` 存在新增的不一致，`2` 你配置或者输入有问题。

`attest extract` 默认只记那种当场就能绑定成功的东西。开了 `--llm` 的话，attest 会调 OpenAI 的 Responses API 再帮你挖一轮，默认模型是 `gpt-5.6-terra`，可以设 `ATTEST_OPENAI_MODEL` 和 `OPENAI_BASE_URL` 来换。LLM 提的每一个锚点都得能确定性绑定，有一个对不上整条 claim 就不要。你把这些候选审一遍，觉得靠谱的把状态改成 `approved`，之后每次 `check` 都会盯住它——来源文档删了就是 broken，内容变了就是 suspect。lock 里要是出现不明的字段、空锚点或者非法的哈希值，attest 会直接拒掉。

GitHub Actions 里用这个仓库的 `action.yml` 就行，默认生成 PR 批注。Claude Code 的 `/attest` skill 和一个可选的 SessionStart hook 在 `.claude/skills/attest/` 里。

## 配置文件

所有配置都能不写。需要定制的话，在仓库根目录放一个 `attest.toml`：

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

## 哪些情况不会误报

attest 的设计原则是拿不准就不说话，宁可漏一千也不错杀一个。

文档里看到"不要"、"禁止"、"暂未实现"这类说法，或者在举例、打比方的时候提了个文件路径——attest 读得懂这是在说"别这样"或者"比如这样"，不是真的断言这个文件存在，不会报错。分支名前缀（"branch 必须以 `claude/` 开头"）、上游仓库的文件（"upstream 的某某已并入本仓库的别处"）、被替代的东西（"X 由 Y 处理了"）也一样——句子自己说明了缺席是应该的。token 出现在标题或者表格行里也一样，那是排版需要，不是说东西一定在。外部链接、页内锚点从一开始就不算断言，只有指向本仓库文件的链接才会去对质。SKILL.md 里引的脚本名和包名也特殊对待，因为 skill 教的是目标仓库怎么干活，不一定是本仓库里有的东西。如果 token 本身长得就像占位符——比如 `your-project/`、`YYYY-MM-DD-notes.md`、`path/to/something`——attest 看一眼形状就知道这不是真实路径，不会追着问。`node_modules`、`target`、`dist` 这些运行时产物和缓存目录同理，跳过。

SKILL.md 有一条专属规则：根级裸文件名（`llms.txt`、`summary.json` 这种没带目录的）在仓库里毫无踪迹时只提醒不报错——skill 教的是 agent 去目标仓库干活，这类名字多半是用户仓库该有的文件或者跑完才生成的产物。带目录的路径不受影响，skill 自带的 references/ 附件缺了照样红。"每次运行会写出"、"构建会 emit"这类产出句式里的文件同理只提醒。

总之每种降级都有明确的理由，写在报告里，不会让你猜"为什么这个没报"。

## 安全

attest 默认只读文件系统和 Git 元数据，不联网。文档里写的命令绝不会被执行。LLM 不走 CI 路径，每次运行结果都可以复现。只有你主动跑 `extract --llm` 的时候才会联网，而且 API key 不会写进 lock 文件或报告里。

更详细的设计、精度纪律和路线图见 `docs/README.md`。公开仓库的 top-50 扫描数据、冷启动验收结果、语料分析、launch 草稿、上游补丁、人工复核记录和完整审计，都在 `reports/`、`corpus/` 和 `demo/` 里。
