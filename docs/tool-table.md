# 第三方工具表贡献规范

`crates/attest-cli/data/tools.json` 是命令检查模块唯一的第三方工具事实来源。它只记那些稳定的、能在官方 CLI 文档里确认的一级子命令。不记项目脚本、shell alias、实验性参数，也不记从本机 `--help` 临时抓到的结果。

## 数据格式

```json
{
  "tool": {
    "subcommands": ["check", "run"],
    "renamed": {"old-command": "new-command"}
  }
}
```

`subcommands` 必须去重并按字典序排好。`renamed` 只在旧命令已经没了、且替代名称唯一的时候加，它只会产生 suspect 提醒，不会报 broken。不确定的、跟版本绑定的、插件提供的命令不往表里放。没进表的命令，检查模块会退回到沉默或者只确认工具本身存在。

## PR 要满足的

PR 描述里链接到这个工具的官方命令文档，说明新增、删除或改名的依据。只改 JSON 数据和相关的 golden test，不要为了某个生态往检查模块里写特殊逻辑。提交前把下面三个跑通：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p attest-cli -- check
```

这个通道遵守"拿不准就不说话"的原则——表里少了条目只会漏检几条，不会报假错。没有官方证据的时候别为了凑覆盖率硬猜。
