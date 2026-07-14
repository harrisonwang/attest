import json
import tempfile
import unittest
from pathlib import Path

from render_release_template import load_checksums, render


ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = [
    "attest-linux-x86_64.tar.gz",
    "attest-linux-aarch64.tar.gz",
    "attest-macos-x86_64.tar.gz",
    "attest-macos-aarch64.tar.gz",
    "attest-windows-x86_64.zip",
    "attest-windows-aarch64.zip",
]


class ReleaseTemplateTests(unittest.TestCase):
    def checksums(self) -> dict[str, str]:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "SHA256SUMS.txt"
            path.write_text(
                "".join(f"{'0' * 64}  {artifact}\n" for artifact in ARTIFACTS),
                encoding="utf-8",
            )
            return load_checksums(path)

    def variables(self) -> dict[str, str]:
        return {
            "BASE_URL": "https://github.com/harrisonwang/attest/releases/download/v0.1.0",
            "CLASS_NAME": "Attest",
            "REPO": "harrisonwang/attest",
            "TOOL": "attest",
            "VERSION": "0.1.0",
        }

    def test_homebrew_template_covers_unix_artifacts(self) -> None:
        template = (ROOT / ".github/homebrew/formula.rb.tmpl").read_text(encoding="utf-8")
        rendered = render(template, self.variables(), self.checksums())

        self.assertIn("class Attest < Formula", rendered)
        for artifact in ARTIFACTS[:4]:
            self.assertIn(artifact, rendered)
        self.assertNotIn("{{", rendered)

    def test_scoop_template_covers_windows_artifacts(self) -> None:
        template = (ROOT / ".github/scoop/attest.json.tmpl").read_text(encoding="utf-8")
        rendered = render(template, self.variables(), self.checksums())
        manifest = json.loads(rendered)

        self.assertEqual(manifest["version"], "0.1.0")
        self.assertIn(ARTIFACTS[4], manifest["architecture"]["64bit"]["url"])
        self.assertIn(ARTIFACTS[5], manifest["architecture"]["arm64"]["url"])

    def test_missing_checksum_fails_closed(self) -> None:
        with self.assertRaisesRegex(KeyError, "missing checksum"):
            render(
                "{{SHA256:missing.zip}}",
                self.variables(),
                self.checksums(),
            )

    def test_checksum_paths_are_keyed_by_asset_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "SHA256SUMS.txt"
            path.write_text(
                f"{'0' * 64}  dist/{ARTIFACTS[0]}\n",
                encoding="utf-8",
            )

            self.assertEqual(load_checksums(path), {ARTIFACTS[0]: "0" * 64})


if __name__ == "__main__":
    unittest.main()
