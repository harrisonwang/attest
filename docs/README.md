# attest 方案文档

> **attest：文档回归测试。文档里声明的，仓库里必须成立。**
>
> 从 markdown 中机械提取 token（反引号、bash fence），逐个绑定到仓库的命名空间（路径 / scripts / 包名 / 命令 / 环境变量 / 符号 / 配置键）——**分类靠绑定，不靠理解，仓库本身就是判定器**。曾经成立的绑定不再成立，即为 drift。CI 内零 LLM、毫秒级、可复现；误差只朝沉默，永不误红。
>
> 楔子：CLAUDE.md / AGENTS.md / SKILL.md——agent 时代文档是承重构件，过时即故障。

| 文档 | 内容 | 状态 |
|------|------|------|
| [00-vision.md](00-vision.md) | 定位、论题、竞争地图（agents-lint/Swimm/…）、护城河、命名事实核查、风险 | 立项方案 |
| [01-design.md](01-design.md) | 设计公理、绑定模型、resolver 规格、作用域、verdict、基线棘轮、claims.lock 与银档、报告协议 attest.report.v1、架构 | 立项方案 |
| [02-roadmap.md](02-roadmap.md) | P0–P4、kill criteria、分发与 launch 策略、不做清单、成功指标 | P0–P3 本地交付完成 |
| [03-probe-2026-07-09.md](03-probe-2026-07-09.md) | 立项实证：5 份真实文件的绑定率数据、六个教训到设计决策的映射 | 已归档 |
| [04-implementation-status.md](04-implementation-status.md) | 当前代码覆盖、验收命令、尚需外部执行的阶段 | 持续更新 |
| [release.md](release.md) | v0.1 一次性发布引导、OIDC、六平台 release、Action 与渠道验收 | 已就绪 |
| [demo-attest-fix.md](demo-attest-fix.md) | `/attest-fix` 检测—修复—复跑录制与 65 秒 H.264 素材 | 已录制 |
| [tool-table.md](tool-table.md) | vendored 第三方命令表的数据结构、精度纪律与 PR 验收 | 已实现 |
| [probe/drift-probe.py](probe/drift-probe.py) | 探针脚本原件（含已知缺陷注记） | 已归档 |

P0–P3 的可本地实现部分已落地；当前剩余工作是按 [04-implementation-status.md](04-implementation-status.md) 执行带凭据的发布、上游 PR 与发帖。P4 继续按真实需求信号牵引，不作为 v0.1 门禁。
