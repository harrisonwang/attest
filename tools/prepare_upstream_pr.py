#!/usr/bin/env python3
"""Validate and optionally apply one reviewed upstream patch."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REMOTE_PATTERNS = (
    re.compile(r"(?:https?://)?github\.com/([^/]+/[^/]+?)(?:\.git)?$"),
    re.compile(r"(?:[^@]+@)?github\.com:([^/]+/[^/]+?)(?:\.git)?$"),
)


def command(clone: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=clone,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def remote_slug(clone: Path) -> str | None:
    result = command(clone, "git", "remote", "get-url", "origin")
    if result.returncode != 0:
        return None
    remote = result.stdout.strip().rstrip("/")
    for pattern in REMOTE_PATTERNS:
        if match := pattern.search(remote):
            return match.group(1).removesuffix(".git")
    return None


def load_submission(manifest: Path, repository: str) -> dict[str, object]:
    payload = json.loads(manifest.read_text(encoding="utf-8"))
    matches = [
        item
        for item in payload.get("submissions", [])
        if item.get("repository") == repository
    ]
    if len(matches) != 1:
        raise ValueError(f"expected one submission for {repository}, found {len(matches)}")
    return matches[0]


def validate_clone(clone: Path, submission: dict[str, object], patch: Path) -> None:
    repository = str(submission["repository"])
    if remote_slug(clone) != repository:
        raise ValueError(f"clone origin does not match {repository}")
    status = command(clone, "git", "status", "--porcelain")
    if status.returncode != 0 or status.stdout:
        raise ValueError("clone must have a clean working tree")
    head = command(clone, "git", "rev-parse", "HEAD")
    if head.returncode != 0 or head.stdout.strip() != submission["snapshot"]:
        raise ValueError(
            f"clone HEAD {head.stdout.strip() or 'unknown'} does not match "
            f"pinned snapshot {submission['snapshot']}"
        )
    check = command(clone, "git", "apply", "--check", str(patch))
    if check.returncode != 0:
        raise ValueError(check.stderr.strip() or "git apply --check failed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repository")
    parser.add_argument("--clone", required=True, type=Path)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=ROOT / "reports/upstream-submissions.json",
    )
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    try:
        submission = load_submission(args.manifest, args.repository)
        patch = ROOT / str(submission["patch"])
        validate_clone(args.clone.resolve(), submission, patch)
        if args.apply:
            applied = command(args.clone.resolve(), "git", "apply", str(patch))
            if applied.returncode != 0:
                raise ValueError(applied.stderr.strip() or "git apply failed")
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"prepare_upstream_pr.py: {error}", file=sys.stderr)
        return 1

    print(f"# {submission['title']}\n")
    print(submission["body"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
