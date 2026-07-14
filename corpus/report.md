# Drift corpus report — 2026-07-14

## Method

`tools/mine-drift.py` scanned local Git histories for a code-changing commit followed by a documentation-only commit that replaced inline-code tokens. Candidates were paired mechanically. A path case received `reviewed: true` only when Git object snapshots proved both conditions:

1. the old path did not exist immediately before the documentation fix;
2. the replacement path existed in the documentation-fix commit.

Private remotes and local-only repositories were excluded from the checked-in corpus with `--github-only`.

## Result

| Metric | Value |
|---|---:|
| Repositories scanned locally | 79+ |
| Raw candidates | 8,557 |
| Public snapshot-verified cases | 280 |
| Public repositories represented | 15 |
| Resolver distribution | path: 280 |

The checked-in `corpus/reviewed.jsonl` contains only the 280 public, snapshot-verified cases from 15 repositories. The Rust corpus test requires every corrected token to bind and at least 80% of stale tokens to produce broken or suspect. Source locators such as `file.rs:42` or `file.sh:function()` count as detected when the file survives but the locator cannot be deterministically reconfirmed; they remain `suspect`, never `broken`.

## Candidate frequency distribution

To avoid using the path-only gold set as a frequency estimate, `corpus/candidates.jsonl` separately records **1,464** mechanically paired inline-code replacements from **10** public repositories, **114** documents, and **146** documentation commits with deep local history. The source file is digest-bound by `corpus/candidate-distribution.json`. These rows are directional mining evidence, not precision gold: a nearby code commit and a later documentation-only replacement can still be unrelated.

| Candidate category | Cases | Share |
|---|---:|---:|
| path | 1,074 | 73.4% |
| prose / unresolved | 234 | 16.0% |
| symbol | 78 | 5.3% |
| cmd | 74 | 5.1% |
| env | 3 | 0.2% |
| pkg | 1 | 0.1% |
| script | 0 | 0.0% |
| go-import | 0 | 0.0% |
| config-key | 0 | 0.0% |

All non-path resolver-shaped rows were inspected as a taxonomy check; ambiguous dotted names were retained as symbols rather than manufactured config-key evidence. Zero means this paired-history sample did not observe the category, not that the resolver is unnecessary. The distribution supports the implemented priority: path first, silver/prose second, then command and symbol, with the remaining namespaces justified by field-scan precision rather than frequency claims.

## Gate status

- **Precision gate:** satisfied for the checked-in path set by deterministic Git snapshot evidence; no heuristic-only candidate is marked reviewed.
- **Coverage gate:** enforced in CI at the P3 target of 80% for reviewed path cases.
- **200-case target:** satisfied (280/200) without private or synthetic cases.
- **Source breadth:** satisfied at the documented lower bound (15 public repositories).
- **Category analysis:** satisfied directionally by the separate digest-bound candidate set; only snapshot-proven rows enter the precision gate.
