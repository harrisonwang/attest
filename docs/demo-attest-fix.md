# `/attest-fix` demo 录制脚本

目标：用 60–90 秒展示“代码已经变、文档还在说旧事实”从检测到自动修复再到复跑变绿的完整闭环。

## 场景

1. 在演示仓库中让 `AGENTS.md` 继续引用已经重命名的 `src/legacy_auth.rs`。
2. 运行 `attest check --format json`，展示该路径以 `broken` 出现，evidence 记录已检查的确定性作用域。
3. 在 Claude Code 中运行 `/attest-fix`；agent 从仓库事实确认当前文件为 `src/auth.rs`，只修改 `AGENTS.md`，不改代码，也不更新 baseline。
4. 展示文档 diff：`src/legacy_auth.rs` → `src/auth.rs`。
5. 再运行 `attest check`，以 `0 broken` 和退出码 `0` 收尾。

## 可执行夹具

仓库已包含同一场景的确定性夹具。它在临时 Git 仓库中完成首次失败、最小文档替换、diff 和最终复跑，不修改项目工作树：

```bash
cargo build --release --locked -p attest-cli
bash demo/attest-fix/run.sh
```

录屏时可增加段间停顿，并在结束后保留临时仓库：

```bash
DEMO_PAUSE_SECONDS=3 KEEP_ATTEST_DEMO=1 bash demo/attest-fix/run.sh
```

脚本中的确定性替换等价于 `/attest-fix` 在该单一 finding 上应执行的最小编辑；正式录制时在首次报告后由 Claude Code 运行 skill，再继续展示同一 diff 与最终复跑。CI 使用无停顿模式证明红→绿闭环始终可复现。

## 已录制素材

仓库同时保存由真实夹具输出生成的 65 秒终端录制与 MP4：

- `demo/attest-fix/attest-fix.cast`：asciinema v2 事件流，可交互回放。
- `demo/attest-fix/attest-fix.mp4`：1280×720、H.264、无音轨的发布素材。

重新录制并渲染：

```bash
python3 demo/attest-fix/record.py --binary target/release/attest
```

录制器会先真实执行红→绿夹具，缺少首次 broken、最小 diff 或最终绿灯时直接失败；MP4 渲染需要 Pillow 与 ffmpeg。事件时间被排布到 65 秒，保持在计划的 60–90 秒窗口内。

## 录制纪律

- 开始前清空终端，保持命令、finding、文档 diff 和最终统计在同一窗口可读。
- 不剪掉首次失败或最终复跑；这两段是“回归测试”而非静态 lint 的证据。
- `suspect` 只称为人工复核项，不包装成确定错误。
- 发布文案可复用 `reports/launch-post.md`，视频本身不展示 API key、私有仓库或未脱敏路径。
