import importlib.util
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "scan_repos", ROOT / "tools/scan-repos.py"
)
assert SPEC is not None and SPEC.loader is not None
SCAN_REPOS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SCAN_REPOS
SPEC.loader.exec_module(SCAN_REPOS)


class ScanReportTests(unittest.TestCase):
    def test_markdown_report_includes_snapshot(self) -> None:
        result = SCAN_REPOS.ScanResult(
            repository="example/project",
            stars=42,
            commit="0123456789abcdef",
            exit_code=0,
            docs=1,
            tokens=2,
            verified=1,
            broken=0,
            suspect=1,
            silent=0,
            findings=[],
        )

        report = SCAN_REPOS.markdown_report(
            SCAN_REPOS.report_payload([result], "agent")
        )

        self.assertIn("| Snapshot |", report)
        self.assertIn("`0123456789ab`", report)


if __name__ == "__main__":
    unittest.main()
