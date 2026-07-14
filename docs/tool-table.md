# 第三方工具表贡献规范

`crates/attest-cli/data/tools.json` 是 `cmd` resolver 的唯一第三方命令事实源。它只记录稳定、可由官方 CLI 文档确认的一级子命令，不记录项目脚本、shell alias、实验 flag 或从本机 `--help` 临时采集的结果。

## 数据结构

```json
{
  "tool": {
    "subcommands": ["check", "run"],
    "renamed": {"old-command": "new-command"}
  }
}
```

- `subcommands` 必须去重并按字典序排列。
- `renamed` 只在旧名称已经移除且替代名称唯一时添加；它只产生 `suspect` 建议，不产生 `broken`。
- 不确定、版本相关或插件提供的命令保持表外，resolver 会退回沉默或只确认工具本身。

## PR 验收

1. PR 描述链接到该工具的官方命令参考，并说明新增、删除或改名的依据。
2. 只修改 JSON 数据和相关 golden test；新增生态不应向 resolver 写工具特例。
3. 运行 `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo run -p attest-cli -- check`。

这一通道遵守“误差只朝沉默”：缺少表项只会漏检；没有确定官方证据时不得为了覆盖率猜测命令。
