# attest 方案文档

attest 做的事情说起来简单：你文档里写了什么，仓库里就得有什么。它从 Markdown 里把反引号里的词和 shell 代码块里的命令抽出来，去仓库里对对看——这个路径还在不在、这个脚本能不能跑、这个包有没有。对得上就过，对不上就告诉你。整个过程不靠 AI 猜，不执行你的代码，跑一遍毫秒级，拿不准的东西不会瞎报错。

为什么做这个？因为现在 AI 编程工具重度依赖 CLAUDE.md、AGENTS.md、SKILL.md 这些文档。文档写错了，AI 就跟着错。文档过时了，AI 就在已经不存在的路径上费劲。attest 就是让这些文档保持诚实的。

各文档说明：

| 文档 | 内容 | 状态 |
|------|------|------|
| [00-vision.md](00-vision.md) | 为什么做、跟竞品的区别、护城河在哪、有什么风险 | 立项方案 |
| [01-design.md](01-design.md) | 设计原则、怎么绑定和裁决、基线怎么工作、claims.lock 的机制、报告协议、整体架构 | 立项方案 |
| [02-roadmap.md](02-roadmap.md) | 分阶段规划、什么时候该停、怎么分发、不做哪些事、怎么判断做成了 | P0–P3 本地部分已完成 |
| [03-probe-2026-07-09.md](03-probe-2026-07-09.md) | 拿 5 份真实文档做的摸底测试，六个教训如何影响最终设计 | 已归档 |
| [04-implementation-status.md](04-implementation-status.md) | 当前代码进度、怎么跑验收、还需要在外部完成的事情 | 持续更新 |
| [release.md](release.md) | v0.1 发布流程：OIDC、六个平台的构建产物、GitHub Action 和分发渠道验收 | 已就绪 |
| [demo-attest-fix.md](demo-attest-fix.md) | `/attest-fix` 从检测到修复到复跑，录了 65 秒的演示视频 | 已录制 |
| [tool-table.md](tool-table.md) | 内置的第三方命令知识库的结构和精度要求 | 已实现 |
| [probe/drift-probe.py](probe/drift-probe.py) | 探针脚本原始版本，标注了已知的缺陷 | 已归档 |

P0 到 P3 能在本地验证的部分都已经做完了。剩下的是需要凭据和外部环境的步骤：正式发布、往上游提 PR、发帖推广，按 `04-implementation-status.md` 里的清单执行就行。P4 之后的事情看真实需求再说，不卡 v0.1 的发布。
