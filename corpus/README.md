# attest drift corpus

该目录保存从 Git 历史中挖出的文档漂移案例。案例使用 JSONL，每行遵循 `attest.corpus.v1`：

```json
{"schema":"attest.corpus.v1","repo":"owner/name","code_commit":"abc123","doc_commit":"def456","doc":"AGENTS.md","before":"pnpm run test:e2e","after":"pnpm run test:e2e-ci","resolver":"script","delay_seconds":86400,"code_subject":"rename e2e script","doc_subject":"fix docs","reviewed":true}
```

语料不收录仓库源码，只保存复核 drift 所需的最小元数据和 token。`reviewed: false` 是待人工确认的高置信候选，不计入精度门禁；只有 `reviewed: true` 才是发布回归语料。运行 `tools/mine-drift.py --help` 查看本地挖矿方式。

目录内有两层证据：

- `reviewed.jsonl`：280 条公开 Git 快照确认的 path drift，是精度与覆盖发布门禁。
- `candidates.jsonl`：10 个公开仓库深历史中的 1,464 条机械配对候选，用于 resolver 频率分析；它包含误配可能，不能当作金档。

`candidate-distribution.json` 保存候选文件 SHA-256、总体统计和逐仓分类计数。用相同仓库快照重跑时可通过：

```bash
python3 tools/mine-drift.py <repo>... \
  --all-markdown \
  --github-only \
  --output corpus/candidates.jsonl \
  --summary corpus/candidate-distribution.json
```
