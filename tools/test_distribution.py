import json
import re
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_TARGETS = {
    "darwin-arm64": ("macos-latest", "attest-macos-aarch64.tar.gz"),
    "darwin-x64": ("macos-15-intel", "attest-macos-x86_64.tar.gz"),
    "linux-arm64": ("ubuntu-24.04-arm", "attest-linux-aarch64.tar.gz"),
    "linux-x64": ("ubuntu-latest", "attest-linux-x86_64.tar.gz"),
    "win32-arm64": ("windows-11-arm", "attest-windows-aarch64.zip"),
    "win32-x64": ("windows-latest", "attest-windows-x86_64.zip"),
}


class DistributionTests(unittest.TestCase):
    def test_npm_targets_match_release_matrix(self) -> None:
        targets = json.loads(
            (ROOT / "packages/npm/platforms.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            targets,
            {target: artifact for target, (_, artifact) in EXPECTED_TARGETS.items()},
        )

        workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        matrix = {
            artifact: runner
            for runner, artifact in re.findall(
                r"- runner: ([^\n]+)\n\s+artifact: ([^\n]+)", workflow
            )
        }
        self.assertEqual(
            matrix,
            {artifact: runner for runner, artifact in EXPECTED_TARGETS.values()},
        )
        self.assertIn("id-token: write", workflow)
        self.assertIn("python3 tools/validate_release.py --tag", workflow)
        self.assertIn("package-manager-cache: false", workflow)
        self.assertIn("dtolnay/rust-toolchain@stable", workflow)
        self.assertIn('npm view "${package}@${version}" version', workflow)
        self.assertNotIn("NODE_AUTH_TOKEN", workflow)
        self.assertNotIn("secrets.NPM_TOKEN", workflow)
        self.assertNotIn("npm version", workflow)

    def test_package_and_workspace_versions_match(self) -> None:
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        package = json.loads(
            (ROOT / "packages/npm/package.json").read_text(encoding="utf-8")
        )

        self.assertEqual(package["version"], workspace["workspace"]["package"]["version"])
        self.assertEqual(set(package["files"]), {"bin", "install.js", "platforms.json"})
        self.assertEqual(package["bin"], {"attest": "bin/attest.js"})

    def test_action_and_installer_use_published_package_contract(self) -> None:
        action = (ROOT / "action.yml").read_text(encoding="utf-8")
        standalone_action = (
            ROOT / "distribution/attest-action/action.yml"
        ).read_text(encoding="utf-8")
        standalone_readme = (
            ROOT / "distribution/attest-action/README.md"
        ).read_text(encoding="utf-8")
        installer = (ROOT / "packages/npm/install.js").read_text(encoding="utf-8")
        action_test = (
            ROOT / "distribution/attest-action/.github/workflows/test.yml"
        ).read_text(encoding="utf-8")

        self.assertEqual(action, standalone_action)
        self.assertEqual(
            (ROOT / "LICENSE").read_text(encoding="utf-8"),
            (ROOT / "distribution/attest-action/LICENSE").read_text(encoding="utf-8"),
        )
        self.assertIn('"@harrisonwang/attest@${ATTEST_VERSION}"', action)
        self.assertIn("harrisonwang/attest-action@v0.1.0", standalone_readme)
        self.assertIn("version: 0.1.0", action_test)
        self.assertIn("fixtures/clean/AGENTS.md", action_test)
        self.assertIn('require("./platforms.json")', installer)
        self.assertIn("releases/download/v${version}", installer)
        self.assertIn("checksum mismatch", installer)


if __name__ == "__main__":
    unittest.main()
