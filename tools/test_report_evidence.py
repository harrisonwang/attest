import hashlib
import json
import unittest
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ReportEvidenceTests(unittest.TestCase):
    def test_vendored_tool_table_is_sorted_and_self_consistent(self) -> None:
        table = json.loads(
            (ROOT / "crates/attest-cli/data/tools.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            set(table),
            {"git", "cargo", "pnpm", "npm", "yarn", "uv", "go", "make", "docker", "gh", "wrangler"},
        )
        for spec in table.values():
            commands = spec["subcommands"]
            self.assertEqual(commands, sorted(set(commands)))
            self.assertTrue(
                all(replacement in commands for replacement in spec.get("renamed", {}).values())
            )

    def test_public_scan_is_snapshot_pinned_and_consistent(self) -> None:
        scan = json.loads((ROOT / "reports/public-scan.json").read_text(encoding="utf-8"))
        stats = scan["stats"]
        repositories = scan["repositories"]

        self.assertEqual(stats["successful"], 50)
        self.assertEqual(stats["failed"], 0)
        self.assertEqual(len(repositories), 50)
        self.assertTrue(all(len(row["commit"]) == 40 for row in repositories))
        for field in ("docs", "tokens", "verified", "broken", "suspect", "silent"):
            self.assertEqual(stats[field], sum(row[field] for row in repositories))
        for row in repositories:
            self.assertEqual(row["broken"], len(row["findings"]))
            self.assertTrue(
                all(finding["verdict"] == "broken" for finding in row["findings"])
            )

    def test_cold_start_acceptance_matches_machine_report(self) -> None:
        expected = {
            "nextdns/nextdns": ("cd213c985a44", 0),
            "sei-protocol/sei-chain": ("959d1275600b", 0),
            "gap-system/gap": ("afe80eeb2aaf", 0),
            "woocommerce/woocommerce-gateway-stripe": ("87a684d18333", 9),
            "1Password/SCAM": ("ce2761e85301", 0),
        }
        scan = json.loads(
            (ROOT / "reports/cold-start-validation.json").read_text(encoding="utf-8")
        )
        repositories = {row["repository"]: row for row in scan["repositories"]}
        reviewed = (ROOT / "reports/reviewed-findings.md").read_text(encoding="utf-8")
        validation = (ROOT / "reports/cold-start-validation.md").read_text(encoding="utf-8")

        self.assertEqual(scan["stats"]["successful"], 5)
        self.assertEqual(scan["stats"]["failed"], 0)
        self.assertEqual(scan["stats"]["broken"], 9)
        self.assertEqual(set(repositories), set(expected))
        for repository, (snapshot, broken) in expected.items():
            row = repositories[repository]
            self.assertTrue(row["commit"].startswith(snapshot))
            self.assertEqual(row["broken"], broken)
            self.assertEqual(len(row["findings"]), broken)
            self.assertTrue(all(finding["verdict"] == "broken" for finding in row["findings"]))
            self.assertIn(f"`{repository}`", validation)

        self.assertIn("Open Pencil", validation)
        self.assertIn("diffblue/cbmc", validation)
        self.assertNotIn("woocommerce/woocommerce-gateway-stripe", reviewed)

    def test_owned_repository_validation_is_complete_and_anonymous(self) -> None:
        validation = json.loads(
            (ROOT / "reports/owned-repository-validation.json").read_text(
                encoding="utf-8"
            )
        )
        stats = validation["stats"]

        self.assertEqual(validation["schema"], "attest.owned-validation.v1")
        self.assertEqual(stats["repositories"], 30)
        self.assertEqual(stats["successful"], 30)
        self.assertEqual(stats["failed"], 0)
        self.assertEqual(stats["clean"] + stats["with_broken"], 30)
        self.assertNotIn("repositories", validation)
        self.assertEqual(validation["binary"]["version"], "attest 0.1.0")
        self.assertEqual(len(validation["binary"]["sha256"]), 64)

    # 提交 PR 时上游 head 已经离开扫描快照，金档改钉在各自的提交基线上，
    # 所以 submissions.json 是唯一权威；扫描报告只用来核对成员资格。
    # react 在扫描后迁移了组织，按别名对回扫描时的名字。
    SCAN_ALIASES = {"react/react": "facebook/react"}

    def test_gold_queue_rows_match_submission_bases(self) -> None:
        submissions = json.loads(
            (ROOT / "reports/upstream-submissions.json").read_text(encoding="utf-8")
        )["submissions"]
        scan = json.loads((ROOT / "reports/public-scan.json").read_text(encoding="utf-8"))
        scanned = {row["repository"] for row in scan["repositories"]}
        reviewed = (ROOT / "reports/reviewed-findings.md").read_text(encoding="utf-8")

        self.assertEqual(len(submissions), 10)
        for submission in submissions:
            repository = submission["repository"]
            snapshot = submission["snapshot"][:12]
            self.assertIn(self.SCAN_ALIASES.get(repository, repository), scanned)
            self.assertIn(f"| `{repository}` | `{snapshot}` |", reviewed)
            self.assertTrue(
                submission["pull_request"].startswith(
                    f"https://github.com/{repository}/pull/"
                )
            )

    def test_gold_queue_has_one_exported_patch_per_repository(self) -> None:
        patch_dir = ROOT / "reports/upstream-patches"
        manifest = (patch_dir / "README.md").read_text(encoding="utf-8")
        patches = {path.name for path in patch_dir.glob("*.patch")}
        submissions = json.loads(
            (ROOT / "reports/upstream-submissions.json").read_text(encoding="utf-8")
        )["submissions"]

        self.assertEqual(len({item["repository"] for item in submissions}), len(submissions))
        self.assertEqual(
            patches, {Path(item["patch"]).name for item in submissions}
        )
        for submission in submissions:
            repository = submission["repository"]
            snapshot = submission["snapshot"][:12]
            name = Path(submission["patch"]).name
            content = (patch_dir / name).read_text(encoding="utf-8")
            self.assertIn("diff --git a/", content)
            self.assertIn(f"| `{repository}` | `{snapshot}` | `{name}` |", manifest)
            self.assertEqual(submission["patch"], f"reports/upstream-patches/{name}")
            self.assertIn("attest", submission["body"])

    def test_checked_in_corpus_is_public_reviewed_path_evidence(self) -> None:
        rows = [
            json.loads(line)
            for line in (ROOT / "corpus/reviewed.jsonl").read_text(encoding="utf-8").splitlines()
        ]

        self.assertGreaterEqual(len(rows), 200)
        self.assertTrue(all(row["schema"] == "attest.corpus.v1" for row in rows))
        self.assertTrue(all(row["reviewed"] for row in rows))
        self.assertTrue(all(row["resolver"] == "path" for row in rows))
        self.assertTrue(all("github.com" in row["repo"] for row in rows))
        self.assertGreaterEqual(len({row["repo"] for row in rows}), 15)

    def test_candidate_corpus_distribution_matches_digest_and_rows(self) -> None:
        candidate_path = ROOT / "corpus/candidates.jsonl"
        contents = candidate_path.read_text(encoding="utf-8")
        rows = [json.loads(line) for line in contents.splitlines()]
        summary = json.loads(
            (ROOT / "corpus/candidate-distribution.json").read_text(
                encoding="utf-8"
            )
        )
        stats = summary["stats"]
        expected_repositories = {
            "https://github.com/K-Dense-AI/claude-scientific-skills.git",
            "https://github.com/alirezarezvani/claude-skills.git",
            "https://github.com/anthropics/skills.git",
            "https://github.com/cloudflare/skills.git",
            "https://github.com/daymade/claude-code-skills.git",
            "https://github.com/huggingface/skills.git",
            "https://github.com/secondsky/claude-skills.git",
            "https://github.com/supabase/agent-skills.git",
            "https://github.com/trailofbits/skills.git",
            "https://github.com/vercel-labs/skills.git",
        }

        self.assertEqual(summary["schema"], "attest.corpus-summary.v1")
        self.assertEqual(
            summary["candidate_sha256"], hashlib.sha256(contents.encode()).hexdigest()
        )
        self.assertEqual(stats["cases"], 1464)
        self.assertEqual(stats["cases"], len(rows))
        self.assertEqual(stats["reviewed"], sum(row["reviewed"] for row in rows))
        self.assertEqual(
            stats["documents"], len({(row["repo"], row["doc"]) for row in rows})
        )
        self.assertEqual(stats["doc_commits"], len({row["doc_commit"] for row in rows}))
        self.assertEqual({row["repo"] for row in rows}, expected_repositories)
        counts = Counter(row["resolver"] for row in rows)
        self.assertEqual(
            summary["resolvers"],
            {category: counts[category] for category in summary["resolvers"]},
        )
        self.assertTrue(all(row["schema"] == "attest.corpus.v1" for row in rows))
        self.assertTrue(all(row["before"] != row["after"] for row in rows))
        self.assertTrue(
            all(len(row["code_commit"]) == len(row["doc_commit"]) == 40 for row in rows)
        )


if __name__ == "__main__":
    unittest.main()
