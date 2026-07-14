import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "scan_owned_repos", ROOT / "tools/scan_owned_repos.py"
)
assert SPEC and SPEC.loader
SCAN_OWNED = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SCAN_OWNED
SPEC.loader.exec_module(SCAN_OWNED)


class OwnedRepositoryScanTests(unittest.TestCase):
    def test_discovery_is_immediate_and_honors_exclusions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            included = root / "included"
            excluded = root / "excluded"
            nested = root / "parent" / "nested"
            for repository in (included, excluded, nested):
                (repository / ".git").mkdir(parents=True)

            repositories = SCAN_OWNED.discover_repositories(
                root, {excluded.resolve()}
            )

            self.assertEqual(repositories, [included.resolve()])

    def test_report_is_aggregate_and_privacy_preserving(self) -> None:
        results = [
            SCAN_OWNED.ScanResult(0, 2, 10, 7, 0, 1, 2),
            SCAN_OWNED.ScanResult(1, 1, 5, 2, 3, 0, 0),
            SCAN_OWNED.ScanResult(2, 0, 0, 0, 0, 0, 0, "runtime failure"),
        ]

        payload = SCAN_OWNED.report_payload(results, "attest 0.1.0", "a" * 64)

        self.assertEqual(payload["schema"], "attest.owned-validation.v1")
        self.assertEqual(
            payload["stats"],
            {
                "repositories": 3,
                "successful": 2,
                "failed": 1,
                "clean": 1,
                "with_broken": 1,
                "docs": 3,
                "tokens": 15,
                "verified": 9,
                "broken": 3,
                "suspect": 1,
                "silent": 2,
            },
        )
        self.assertNotIn("repositories", payload)
        self.assertNotIn("runtime failure", str(payload))


if __name__ == "__main__":
    unittest.main()
