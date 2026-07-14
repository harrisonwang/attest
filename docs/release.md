# v0.1 release runbook

This runbook separates one-time bootstrap work from the repeatable tag release. Public side effects require explicit approval before execution.

## One-time bootstrap

1. Create the public `harrisonwang/attest` repository and push the reviewed `main` branch.
2. Create the public `harrisonwang/attest-action` repository from `distribution/attest-action/`; tag its reviewed initial commit `v0.1.0` only after the npm package is installable.
3. Configure repository secrets in `harrisonwang/attest`:
   - `HOMEBREW_TAP_TOKEN`: token allowed to dispatch `harrisonwang/homebrew-tap`.
   - `SCOOP_BUCKET_TOKEN`: token allowed to dispatch `harrisonwang/scoop-bucket`.
   - `NPM_TOKEN`: required for the first package publication and retained only as an emergency fallback.
4. After `@harrisonwang/attest` exists on npm, configure its [trusted publisher](https://docs.npmjs.com/trusted-publishers/) with organization/user `harrisonwang`, repository `attest`, workflow `release.yml`, and permission to run `npm publish`.
5. Verify tag protection and remove the long-lived npm token after one OIDC-backed release succeeds.

The release workflow grants `id-token: write` only to the publish job. npm 11 on Node 24 uses that OIDC identity when the trusted publisher is configured and otherwise falls back to `NPM_TOKEN` for the bootstrap release.

## Prepare a release

1. Update `[workspace.package].version` in `Cargo.toml` and `version` in `packages/npm/package.json`.
2. Run `cargo check --workspace` so the two workspace entries in `Cargo.lock` match.
3. Validate all source-controlled release metadata:

   ```bash
   python3 tools/validate_release.py
   ```

4. Run the complete local gate:

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

5. Review `git diff`, commit the version bump, and create an exact matching tag. For version `0.1.0`, the tag must be `v0.1.0`; any mismatch fails before six-platform builds begin.

## Publish

Push the reviewed commit and tag:

```bash
git push origin main
git push origin v0.1.0
```

`.github/workflows/release.yml` then:

1. validates the tag, Cargo/npm versions, lockfile versions, repository identity, public npm access, and channel secrets;
2. builds and packages x86-64 and arm64 binaries for Linux, macOS, and Windows on GitHub-hosted runners;
3. publishes checksums, rendered Homebrew/Scoop manifests, and all archives in a GitHub Release;
4. publishes the npm wrapper using OIDC or the bootstrap token;
5. dispatches the release to the existing Homebrew tap and Scoop bucket automation.

The runner labels are the current standard public-repository labels documented in the [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

## Verify channels

```bash
gh release view v0.1.0 --repo harrisonwang/attest
npm view @harrisonwang/attest version
npx --yes @harrisonwang/attest@0.1.0 --version
brew update && brew install harrisonwang/tap/attest
scoop update && scoop install attest
```

Confirm both native package managers report `attest 0.1.0`, then apply `distribution/attest-action/` to the independent Action repository and verify a fixture workflow with `harrisonwang/attest-action@v0.1.0`.

## Upstream and launch

Before each upstream submission, fetch the repository head, re-run `git apply --check` for its file under `reports/upstream-patches/`, inspect the resulting diff, and rerun Attest on the modified document. Submit only unchanged gold findings. Record real PR URLs in `reports/reviewed-findings.md`, then publish the finalized copy from `reports/launch-post.md`.
