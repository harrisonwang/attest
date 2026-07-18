# attest — 实现进度

> 更新：2026-07-18

## 已经做完了

attest-core 和 attest-cli 搭好了，Rust workspace。core 是纯逻辑层，不碰文件系统不碰网络。cli 按功能分模块——check、extract、llm、vouch、render、store、surface、prose，main 只负责解析参数和分发。

Markdown 提取这块，inline code 和显式标了 bash、sh、shell、zsh、console 的围栏块都能抽，链接和图片的目标也抽——带 scheme 的、页内锚点、绝对路径跳过，剩下的按本仓库文件引用处理，且只走路径检查，不会被脚本、命令这些角度错绑。命令解析支持环境变量前缀、注释、续行拼接、heredoc 跳过。路径、脚本、包名的 `*` 和 `?` 通配符按确定性规则匹配，匹配不到不出声。没标语言的围栏块不产 token，这是刻意取舍——ASCII 大纲和目录树会混进来——代价是这类内容里的漂移工具看不见。

八个检查模块全部到位：路径、脚本、包名、命令、Go import、环境变量、配置键、代码符号。工具子命令表内置了 JSON 数据。表里没有的工具子命令保持沉默，不算 verified。`owner/repo` 这种仓库缩写不做迁移猜测。

token 之外还有一个文档级校验：SKILL.md 的 frontmatter。块缺失、YAML 解析失败、name 或 description 不在或为空算 broken；形状不合规范只提 suspect；name 还是占位符的按模板跳过，references 和 templates 目录同理。开头 `---` 带尾随空白的真实文件也认——这是 2026-07-18 冷启动复核在一个六千文档的 skill 聚合仓库上逮出来的形状，已进回归。同一轮复核在 bun 主干上确认了三条真漂移、七条守卫盲区，盲区全部按"普遍语言现象"标准补进语境守卫（括号里的否定、分支名前缀、upstream 与替代句式），案例钉进 `corpus/guard-cases.jsonl`。

2026-07-18 又拿 AutoGPT、openclaw、marketingskills、agents.md 四个本地仓库做了一轮复核，暴露出契约文档的最大误报类：文档描述"目标仓库的文件"和"运行时产物"。按人工逐条定性后收进三处：SKILL.md 的根级裸文件名无本地踪迹时降为提醒（带目录的附件引用不受影响，bun 的三条真漂移回归确认还在咬）；语境守卫补产出动词（writes、serialize、emit）、否定列举（no 开头）、备选列举（appropriate）；形状守卫补命名元变量（驼峰 Name 收尾、裸 Component 词干）和 ASCII 省略号。四仓库 broken 从 53 降到 6，剩余里 2 条是真问题（AutoGPT 的 coverage-fixture 已搬家、openclaw 的 lobster SKILL.md 没有 frontmatter），其余是"已提交目录里的未 ignore 运行产物"这类守不住的残留，走 baseline 记账。全部形状钉进守卫语料。

文件树采集走 `git ls-files`，尊重 .gitignore。非 git 目录退化成受限的目录遍历。符号和环境变量的搜索首次调用时建好词表索引，之后全查表。文档引用了被 .gitignore 排除的路径，走 `git check-ignore` 查一遍（有缓存），按运行时产物处理——不报 broken 只提醒。

monorepo 的路径查找按文档自身目录、项目根、仓库根的优先级来，可以在 attest.toml 里用 glob 钉死。路径 glob 在 core 里只有一份实现，CLI 和测试替身共用，不会出现测试和生产语义不同的问题。

裁决结果分四级：verified 对上、broken 对不上、suspect 拿不准、silent 不出声。broken 报出前要过四道守卫——结构、语境、文档类别、形状。守卫只看 token 所在的那一行，先遮掉行内代码再匹配。每条降级的理由写在报告里。守卫行为由 `corpus/guard-cases.jsonl` 里的标注语料逐条回归，某个仓库特有的例外只进语料文件不进代码。改法建议只在唯一候选的时候给，多个候选只列出来供人判断。

命令行支持三个子命令：check、baseline update、extract。输出格式有终端 TTY、JSON、GitHub annotations 三种。支持 `--since` 增量检查、`--vouch-ir` 定向模式、`--strict` 严格模式。

存量项目的基线机制做好了。`.attest/baseline.json` 按文档名、token 文本、检查类型稳定匹配，不按行号。修一条少一条，`baseline update` 收紧。

claims.lock 做完了。严格 YAML 格式，支持 proposed 和 approved 两种状态。确定性锚点用 SHA-256 做内容哈希。来源文档或锚点删了报 broken，内容变了只报 suspect。lock 文件用严格 schema 校验，未知字段直接拒绝。

作者时点的 LLM 抽取也做了。走 OpenAI 兼容的 Responses API，严格 Structured Outputs，模型和端点可配。所有锚点必须确定性绑定成功才生成 proposed claim。

vouch IR 的定向模式——读了 Commit IR 之后只查直接变更相关的文档和 lock claims。

分发资产就绪了。npm 零安装包装器和六平台构建清单。composite GitHub Action 和独立仓库导出包。六平台 release workflow，版本和 tag 的 fail-closed 预检。npm OIDC trusted publishing、Homebrew 和 Scoop 模板和自动 dispatch。`/attest` 和 `/attest-fix` 两个 agent skill，加上 SessionStart hook 配方。

语料方面，P0 阶段收了 280 条案例，覆盖 15 个公开仓库，Git 快照确认过的，满足 200 条和 15 到 20 个来源仓库的原始目标。另外 10 个公开的深历史仓库挖出 1,464 条摘要绑定的机械候选，用来做检查模块频率分析，候选不混入精度金档。没有用私有仓库或合成数据填数。检出率双线门禁：broken 率目标 ≥55%、总检出 ≥80%。当前实测 broken 58.6%，总检出 85.0%。

冷启动验收：五个没进过语料也没进过 top-50 扫描的公开仓库，共 9 条 broken 逐条复核过，金档误报为零。证据在 `reports/cold-start-validation.md`。

自有仓库兼容性：release binary 扫了同级 30 个 Git 仓库，27 个 clean、3 个按产品语义报了 drift、0 个运行错误。聚合证据不记录私有仓库身份，见 `reports/owned-repository-validation.md`。

公开仓库扫描：按 GitHub stars 排序，成功扫完了 50 个有 AGENTS 或 CLAUDE 文档的公开仓库，覆盖 330 份文档和 16,310 个 token。结构化报告、launch 草稿和 10 个已经通过 `git apply --check` 的高确认度补丁都落盘了。

修复闭环 demo：`/attest-fix` skill、临时 Git 仓库的红到绿夹具、asciinema 事件流和 65 秒 H.264 视频都做好了。夹具和录制证据进了 CI。

工具表社区通道：第三方命令保持纯 JSON 数据，贡献规范要求给出官方证据、字典序排列、golden test 和完整的门禁。

## 还需要在外部做的事情

npm 和 GitHub Release 的正式发布、把 `distribution/attest-action/` 推成独立仓库、10 个上游 PR 提交、以及 HN、X、中文社区的发布——这些都还需要真实的对外执行。GitHub CLI 和 npm 都已经认证为 `harrisonwang` 了，`npm publish --dry-run --access public` 也跑通了，但主仓库和 Action 仓库还没创建，npm 包还没公开。具体步骤看 `docs/release.md`。

修复闭环的 demo 视频已经真实录制好了，第二波传播属于对外执行。

远期的事情按真实需求牵引，不卡 v0.1 的发布：tree-sitter 符号搜索、路由检查模块、MCP server、VS Code 插件、introspective 执行档。

逐项的证据和未完成的判定在 `reports/completion-audit.md` 里。

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
