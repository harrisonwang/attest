#!/usr/bin/env python3
"""Run attest across local repositories and emit privacy-preserving aggregate evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path


@dataclass(frozen=True)
class ScanResult:
    exit_code: int
    docs: int
    tokens: int
    verified: int
    broken: int
    suspect: int
    silent: int
    error: str | None = None


def discover_repositories(root: Path, excluded: set[Path]) -> list[Path]:
    repositories = []
    for candidate in sorted(root.iterdir()):
        resolved = candidate.resolve()
        if resolved in excluded or not candidate.is_dir():
            continue
        if (candidate / ".git").exists():
            repositories.append(resolved)
    return repositories


def scan_repository(binary: Path, repository: Path, timeout: int) -> ScanResult:
    try:
        process = subprocess.run(
            [str(binary), "--root", str(repository), "check", "--format", "json"],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return ScanResult(124, 0, 0, 0, 0, 0, 0, f"exceeded {timeout}s timeout")

    if process.returncode not in {0, 1}:
        error = process.stderr.strip() or "attest failed without an error message"
        return ScanResult(process.returncode, 0, 0, 0, 0, 0, 0, error)

    try:
        report = json.loads(process.stdout)
        if report["schema"] != "attest.report.v1":
            raise ValueError(f"unexpected schema {report['schema']!r}")
        stats = report["stats"]
        result = ScanResult(
            exit_code=process.returncode,
            docs=int(stats["docs"]),
            tokens=int(stats["tokens"]),
            verified=int(stats["verified"]),
            broken=int(stats["broken"]),
            suspect=int(stats["suspect"]),
            silent=int(stats["silent"]),
        )
    except (json.JSONDecodeError, KeyError, TypeError, ValueError) as error:
        return ScanResult(2, 0, 0, 0, 0, 0, 0, f"invalid attest report: {error}")

    expected_exit = 1 if result.broken else 0
    if result.exit_code != expected_exit:
        return ScanResult(
            result.exit_code,
            0,
            0,
            0,
            0,
            0,
            0,
            f"exit code {result.exit_code} disagrees with broken={result.broken}",
        )
    return result


def binary_metadata(binary: Path) -> tuple[str, str]:
    version = subprocess.run(
        [str(binary), "--version"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    ).stdout.strip()
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    return version, digest


def report_payload(
    results: list[ScanResult], version: str, binary_sha256: str
) -> dict[str, object]:
    successful = [result for result in results if result.error is None]
    return {
        "schema": "attest.owned-validation.v1",
        "generated_at": datetime.now(UTC).isoformat(),
        "scope": "immediate Git repositories; repository identities intentionally omitted",
        "binary": {"version": version, "sha256": binary_sha256},
        "stats": {
            "repositories": len(results),
            "successful": len(successful),
            "failed": len(results) - len(successful),
            "clean": sum(result.exit_code == 0 for result in successful),
            "with_broken": sum(result.exit_code == 1 for result in successful),
            "docs": sum(result.docs for result in successful),
            "tokens": sum(result.tokens for result in successful),
            "verified": sum(result.verified for result in successful),
            "broken": sum(result.broken for result in successful),
            "suspect": sum(result.suspect for result in successful),
            "silent": sum(result.silent for result in successful),
        },
    }


def markdown_report(payload: dict[str, object]) -> str:
    stats = payload["stats"]
    binary = payload["binary"]
    assert isinstance(stats, dict)
    assert isinstance(binary, dict)
    return "\n".join(
        [
            "# Owned-repository compatibility validation",
            "",
            f"> Generated {payload['generated_at']} · `attest.owned-validation.v1`",
            "",
            "Repository identities and findings are intentionally omitted because this is a local compatibility run. A broken exit is an expected product verdict; only timeouts, malformed reports, or runtime failures count as scanner failures.",
            "",
            "| Repositories | Successful | Clean | With broken | Runtime failures | Docs | Tokens | Verified | Broken | Suspect | Silent |",
            "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
            f"| {stats['repositories']} | {stats['successful']} | {stats['clean']} | {stats['with_broken']} | {stats['failed']} | {stats['docs']} | {stats['tokens']} | {stats['verified']} | {stats['broken']} | {stats['suspect']} | {stats['silent']} |",
            "",
            f"Binary: `{binary['version']}` · SHA-256 `{binary['sha256']}`",
            "",
        ]
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discover", type=Path, required=True)
    parser.add_argument("--exclude", action="append", type=Path, default=[])
    parser.add_argument("--binary", type=Path, default=Path("target/release/attest"))
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument(
        "--json", type=Path, default=Path("reports/owned-repository-validation.json")
    )
    parser.add_argument(
        "--markdown",
        type=Path,
        default=Path("reports/owned-repository-validation.md"),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        print(f"attest binary not found: {binary}", file=sys.stderr)
        return 2
    root = args.discover.resolve()
    if not root.is_dir():
        print(f"discovery root not found: {root}", file=sys.stderr)
        return 2
    excluded = {path.resolve() for path in args.exclude}
    repositories = discover_repositories(root, excluded)
    if not repositories:
        print(f"no Git repositories found directly under {root}", file=sys.stderr)
        return 2

    results = []
    for index, repository in enumerate(repositories, start=1):
        result = scan_repository(binary, repository, args.timeout)
        results.append(result)
        status = "ok" if result.error is None else f"error: {result.error}"
        print(f"[{index}/{len(repositories)}] {repository.name}: {status}", file=sys.stderr)

    version, binary_sha256 = binary_metadata(binary)
    payload = report_payload(results, version, binary_sha256)
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    args.markdown.write_text(markdown_report(payload), encoding="utf-8")
    return 0 if payload["stats"]["failed"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
