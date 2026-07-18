# v0.1 发布流程

一次性启动工作和可重复的 tag 发布分开说。公共侧的操作需要确认后再执行。

## 一次性启动

先创建公开的 `harrisonwang/attest` 仓库，把审核过的 main 分支推上去。再从 `distribution/attest-action/` 创建公开的 `harrisonwang/attest-action` 仓库，等 npm 包可安装之后再给初始提交打 `v0.1.0` 的 tag。

在 `harrisonwang/attest` 仓库配好三个 secret。`HOMEBREW_TAP_TOKEN` 用于触发 harrisonwang/homebrew-tap 的 dispatch。`SCOOP_BUCKET_TOKEN` 用于触发 harrisonwang/scoop-bucket 的 dispatch。`NPM_TOKEN` 用于首次发布，之后只当应急备用的。

`@harrisonwang/attest` 上了 npm 之后，配好 trusted publisher：组织或用户填 harrisonwang、仓库填 attest、workflow 填 `.github/workflows/release.yml`、权限给 npm publish。确认 tag 保护开启，等一次 OIDC 支持的发布成功之后删掉长期 npm token。

release workflow 只给 publish job 开 `id-token: write`。npm 11 在 Node 24 上配了 trusted publisher 之后会走 OIDC 身份，没配的话退回到 `NPM_TOKEN` 顶首次发布用。

## 准备发布

更新 `Cargo.toml` 里 `[workspace.package].version` 和 `packages/npm/package.json` 里的 `version`。跑 `cargo check --workspace` 让 `Cargo.lock` 里的两条 workspace 条目对齐。

校验发布元数据：

```bash
python3 tools/validate_release.py
```

跑完整的本地门禁：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --locked -p attest-cli
node packages/npm/test.js
python3 -m unittest discover -s tools -p 'test_*.py'
target/release/attest check
python3 demo/attest-fix/record.py --binary target/release/attest --cast /tmp/attest-fix.cast --no-video
(cd packages/npm && npm pack --dry-run)
```

检查 `git diff`，提交版本号改动，打一个完全匹配的 tag。版本号是 `0.1.0` 的话，tag 必须是 `v0.1.0`。不匹配的话六个平台的构建开始之前就会失败。

## 发布

推审核过的提交和 tag：

```bash
git push origin main
git push origin v0.1.0
```

`.github/workflows/release.yml` 被触发之后做这些事情：校验 tag、Cargo 和 npm 版本号、lockfile 版本、仓库身份、npm 公开访问权限、频道 secret。在 GitHub 托管的 runner 上构建和打包 Linux、macOS、Windows 的 x86-64 和 arm64 二进制。发布校验和、渲染好的 Homebrew 和 Scoop manifest、以及全部归档文件到 GitHub Release。用 OIDC 或启动期 token 发布 npm wrapper。把 release dispatch 到已有的 Homebrew tap 和 Scoop bucket 自动化流程。

runner 标签用的是 GitHub 公开仓库文档里的标准标签。

## 验证分发渠道

```bash
gh release view v0.1.0 --repo harrisonwang/attest
npm view @harrisonwang/attest version
npx --yes @harrisonwang/attest@0.1.0 --version
brew update && brew install harrisonwang/tap/attest
scoop update && scoop install attest
```

确认两个原生包管理器都报 `attest 0.1.0`。然后把 `distribution/attest-action/` 应用到独立的 Action 仓库，用一个测试 workflow 验证 `harrisonwang/attest-action@v0.1.0`。

## 上游 PR 和发布

每次提交上游 PR 之前，拉一下目标仓库的最新代码，重新跑 `git apply --check` 对应 `reports/upstream-patches/` 里的补丁文件，看一眼改动内容，再在改后的文档上跑一遍 attest。只提交确认度不变的东西。真实的 PR 链接记在 `reports/reviewed-findings.md` 里。发布文案用 `reports/launch-post.md` 的定稿。
