# Upstream patch bundle

> Exported 2026-07-14 from the reviewed, commit-pinned launch queue. These files are local preparation artifacts, not submitted pull requests. Rebase and re-review every patch against the upstream head before submission.

| Repository | Snapshot | Patch |
|---|---|---|
| `react/react` | `c0c39a6b3907` | `facebook-react.patch` |
| `oven-sh/bun` | `16c557635bb3` | `oven-sh-bun.patch` |
| `OpenHands/OpenHands` | `5f9906fbdac3` | `openhands-openhands.patch` |
| `continuedev/continue` | `d0a3c0b626b5` | `continuedev-continue.patch` |
| `vercel-labs/skills` | `cf4a3ea678b7` | `vercel-labs-skills.patch` |
| `alirezarezvani/claude-skills` | `8c4a374a443a` | `alirezarezvani-claude-skills.patch` |
| `deanpeters/Product-Manager-Skills` | `99be43c842d3` | `deanpeters-product-manager-skills.patch` |
| `jeremylongshore/claude-code-plugins-plus-skills` | `2d86dfbcf3e2` | `jeremylongshore-claude-code-plugins-plus-skills.patch` |
| `NVIDIA/skills` | `9559272b38d9` | `nvidia-skills.patch` |
| `OpenHands/docs` | `a7d418214914` | `openhands-docs.patch` |

Each patch passed `git apply --check` against its full 40-character snapshot and the modified instruction documents produced zero `broken` findings when rescanned with the release binary.

The Jeremy Longshore patch intentionally fixes only the in-repository Firebase guidance. The Mnemos files are a mirror of `polyxmedia/mnemos`; the target repository's policy requires that documentation fix to land upstream and flow through sync rather than be hand-edited in the mirror.

Apply a patch from a clean checkout with:

```bash
git apply --check /path/to/reports/upstream-patches/<name>.patch
git apply /path/to/reports/upstream-patches/<name>.patch
```
