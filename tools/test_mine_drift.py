import importlib.util
import json
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("mine_drift", ROOT / "tools/mine-drift.py")
assert SPEC and SPEC.loader
MINE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MINE
SPEC.loader.exec_module(MINE)


class MineDriftTests(unittest.TestCase):
    def test_classifier_covers_resolver_shapes(self) -> None:
        expected = {
            "DATABASE_URL": "env",
            "pnpm run test:e2e": "script",
            "hf jobs uv run train.py": "cmd",
            "@attest/core": "pkg",
            "policy.fail-on": "symbol",
            "github.com/acme/service/pkg/api": "go-import",
            "src/auth.rs": "path",
            "report_template.md": "path",
            "verify_claim": "symbol",
            "requires Node 24": "prose",
        }

        self.assertEqual(
            {token: MINE.classify(token) for token in expected}, expected
        )

    def test_summary_is_deterministic_and_self_consistent(self) -> None:
        cases = [
            MINE.Case(
                schema="attest.corpus.v1",
                repo="https://github.com/acme/repo.git",
                code_commit="a" * 40,
                doc_commit="b" * 40,
                doc="AGENTS.md",
                before="src/old.rs",
                after="src/new.rs",
                resolver="path",
                delay_seconds=1,
                code_subject="rename source",
                doc_subject="repair docs",
                reviewed=True,
            )
        ]
        output = "".join(json.dumps(MINE.asdict(case)) + "\n" for case in cases)

        summary = MINE.summary_payload(cases, output)

        self.assertEqual(summary["schema"], "attest.corpus-summary.v1")
        self.assertEqual(summary["stats"]["cases"], 1)
        self.assertEqual(summary["stats"]["reviewed"], 1)
        self.assertEqual(summary["resolvers"]["path"], 1)
        self.assertEqual(summary["resolvers"]["script"], 0)
        self.assertEqual(summary["resolvers"]["config-key"], 0)
        self.assertEqual(len(summary["candidate_sha256"]), 64)


if __name__ == "__main__":
    unittest.main()
