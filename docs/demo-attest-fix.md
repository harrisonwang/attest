# `/attest-fix` demo 录制脚本

目标是用 60 到 90 秒展示完整的闭环——代码已经变了，文档还在说旧的，attest 检测出来，agent 自动修好，再跑一次确认绿了。

## 演示的场景

一个仓库里，`AGENTS.md` 还在引用已经改过名的 `src/legacy_auth.rs`。跑 `attest check --format json`，这条路径以 broken 出现，evidence 里记录了检查过的地方。在 Claude Code 里跑 `/attest-fix`，agent 从仓库事实里确认当前文件名是 `src/auth.rs`，只改文档，不动代码，也不动 baseline。展示 diff：`src/legacy_auth.rs` 变成 `src/auth.rs`。再跑一次 `attest check`，0 条 broken，退出码 0，收工。

## 可复现的自动脚本

仓库里已经放了同场景的自动化夹具。它在临时 Git 仓库里完成首次失败、最小文档替换、diff 和最终复跑，不动项目的实际工作目录。跑一下就行：

```bash
cargo build --release --locked -p attest-cli
bash demo/attest-fix/run.sh
```

录屏的时候可以在段落之间加点停顿，结束后保留临时仓库：

```bash
DEMO_PAUSE_SECONDS=3 KEEP_ATTEST_DEMO=1 bash demo/attest-fix/run.sh
```

脚本里的替换逻辑等价于 `/attest-fix` 在这一个 finding 上该做的最小编辑。正式录制的时候，首次 report 出来之后让 Claude Code 跑 skill，然后继续展示同样的 diff 和最终复跑。CI 里用无停顿模式证明红到绿的闭环每次都能复现。

## 已录好的素材

仓库里存了从真实夹具输出生成的 65 秒终端录制和 MP4。

`demo/attest-fix/attest-fix.cast` 是 asciinema v2 事件流，可以交互回放。`demo/attest-fix/attest-fix.mp4` 是 1280×720、H.264、无音轨的发布素材。

想重新录的话跑这个：

```bash
python3 demo/attest-fix/record.py --binary target/release/attest
```

录制器会先真实跑一遍红到绿夹具，如果缺了首次 broken、最小 diff 或最终绿灯就直接失败，不糊弄。MP4 渲染需要 Pillow 和 ffmpeg。事件时间被排到 65 秒，刚好卡在计划的 60 到 90 秒窗口内。

## 录制要注意的

开始前清空终端，让命令、finding、文档 diff 和最终统计都在同一个窗口里看得清。不要剪掉首次失败和最终复跑，这两段是"回归测试"和"静态 lint"的本质区别。suspect 只叫"人工复核项"，不包装成确定错误。发布文案可以复用 `reports/launch-post.md`。视频里不要出现 API key、私有仓库或者没脱敏的路径。
