import json
import shutil
import tempfile
import tomllib
import unittest
from pathlib import Path

from validate_release import validate_release


ROOT = Path(__file__).resolve().parents[1]


def source_version() -> str:
    with open(ROOT / "Cargo.toml", "rb") as handle:
        return tomllib.load(handle)["workspace"]["package"]["version"]


class ValidateReleaseTests(unittest.TestCase):
    def test_current_release_metadata_is_consistent(self) -> None:
        self.assertEqual(validate_release(ROOT, f"v{source_version()}"), [])

    def test_release_tag_must_match_source_version(self) -> None:
        errors = validate_release(ROOT, "v99.0.0")

        self.assertIn(
            f"release tag v99.0.0 does not match source version v{source_version()}",
            errors,
        )

    def test_npm_version_and_public_access_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            shutil.copy(ROOT / "Cargo.toml", fixture / "Cargo.toml")
            shutil.copy(ROOT / "Cargo.lock", fixture / "Cargo.lock")
            package_dir = fixture / "packages/npm"
            package_dir.mkdir(parents=True)
            package = json.loads(
                (ROOT / "packages/npm/package.json").read_text(encoding="utf-8")
            )
            package["version"] = "0.2.0"
            package.pop("publishConfig", None)
            (package_dir / "package.json").write_text(
                json.dumps(package), encoding="utf-8"
            )

            errors = validate_release(fixture)

        self.assertTrue(any("npm package version" in error for error in errors))
        self.assertIn("npm publishConfig.access must be public", errors)


if __name__ == "__main__":
    unittest.main()
