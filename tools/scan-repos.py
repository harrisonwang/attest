#!/usr/bin/env python3
"""Scan public GitHub repositories with attest and build launch-ready reports."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path

EXCLUDED_DIRS = {
    ".git",
    ".venv",
    ".build",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
}
REMOTE_PATTERNS = (
    re.compile(r"(?:https?://)?github\.com/([^/]+/[^/]+?)(?:\.git)?$"),
    re.compile(r"(?:[^@]+@)?github\.com:([^/]+/[^/]+?)(?:\.git)?$"),
)
STAR_PATTERN = re.compile(
    rb'id="repo-stars-counter-star"[^>]*aria-label="([0-9]+) users? starred this repository"'
)


@dataclass(frozen=True)
class Repository:
    slug: str
    path: Path
    stars: int


@dataclass
class ScanResult:
    repository: str
    stars: int
    commit: str | None
    exit_code: int
    docs: int
    tokens: int
    verified: int
    broken: int
    suspect: int
    silent: int
    findings: list[dict[str, object]]
    error: str | None = None


def command(path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=path,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def remote_slug(repo: Path) -> str | None:
    output = command(repo, "git", "remote", "get-url", "origin")
    if output.returncode != 0:
        return None
    remote = output.stdout.strip().rstrip("/")
    for pattern in REMOTE_PATTERNS:
        if match := pattern.search(remote):
            return match.group(1).removesuffix(".git")
    return None


def target_docs(repo: Path, profile: str) -> list[str]:
    docs = []
    for root, directories, files in os.walk(repo):
        directories[:] = [name for name in directories if name not in EXCLUDED_DIRS]
        root_path = Path(root)
        for name in files:
            relative = (root_path / name).relative_to(repo).as_posix()
            if name in {"AGENTS.md", "CLAUDE.md"}:
                docs.append(relative)
            elif name.endswith(".md") and ".claude" in Path(relative).parts:
                docs.append(relative)
            elif profile == "all" and name == "SKILL.md":
                docs.append(relative)
    return sorted(set(docs))


def discover_repositories(roots: list[Path], profile: str) -> dict[str, Path]:
    repositories: dict[str, Path] = {}
    for search_root in roots:
        for root, directories, files in os.walk(search_root):
            root_path = Path(root)
            if ".git" in directories or ".git" in files:
                directories[:] = [name for name in directories if name != ".git"]
                if target_docs(root_path, profile):
                    if slug := remote_slug(root_path):
                        repositories.setdefault(slug, root_path)
                directories[:] = []
                continue
            directories[:] = [name for name in directories if name not in EXCLUDED_DIRS]
    return repositories


def github_page_stars(slug: str) -> int | None:
    request = urllib.request.Request(
        f"https://github.com/{slug}",
        headers={"User-Agent": "attest-corpus-scanner"},
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            body = response.read()
    except (OSError, urllib.error.HTTPError) as error:
        print(f"skip {slug}: GitHub page unavailable ({error})", file=sys.stderr)
        return None
    match = STAR_PATTERN.search(body)
    if match is None:
        print(f"skip {slug}: GitHub page has no star metadata", file=sys.stderr)
        return None
    return int(match.group(1))


def github_stars(slug: str, token: str | None) -> int | None:
    request = urllib.request.Request(
        f"https://api.github.com/repos/{slug}",
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "attest-corpus-scanner",
            **({"Authorization": f"Bearer {token}"} if token else {}),
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code in {403, 429}:
            print(
                f"rank {slug} from repository page: GitHub API rate limited ({error.code})",
                file=sys.stderr,
            )
            return github_page_stars(slug)
        print(f"skip {slug}: GitHub metadata unavailable ({error})", file=sys.stderr)
        return None
    except (OSError, json.JSONDecodeError) as error:
        print(f"skip {slug}: GitHub metadata unavailable ({error})", file=sys.stderr)
        return None
    stars = payload.get("stargazers_count")
    return stars if isinstance(stars, int) else None


def rank_repositories(
    paths: dict[str, Path], workers: int, cached_stars: dict[str, int]
) -> list[Repository]:
    token = os.environ.get("GITHUB_TOKEN")
    repositories = [
        Repository(slug, path, cached_stars[slug])
        for slug, path in paths.items()
        if slug in cached_stars
    ]
    with ThreadPoolExecutor(max_workers=workers) as executor:
        futures = {
            executor.submit(github_stars, slug, token): (slug, path)
            for slug, path in sorted(paths.items())
            if slug not in cached_stars
        }
        for future in as_completed(futures):
            slug, path = futures[future]
            stars = future.result()
            if stars is not None:
                repositories.append(Repository(slug, path, stars))
    return sorted(repositories, key=lambda repo: (-repo.stars, repo.slug))


def load_cached_stars(path: Path | None) -> dict[str, int]:
    if path is None or not path.is_file():
        return {}
    try:
        payload = json.loads(path.read_text())
        return {
            result["repository"]: result["stars"]
            for result in payload.get("repositories", [])
            if isinstance(result, dict)
            and isinstance(result.get("repository"), str)
            and isinstance(result.get("stars"), int)
        }
    except (OSError, json.JSONDecodeError, TypeError):
        return {}


def scan_repository(
    binary: Path, repository: Repository, timeout: int, profile: str
) -> ScanResult:
    docs = target_docs(repository.path, profile)
    revision = command(repository.path, "git", "rev-parse", "HEAD")
    commit = revision.stdout.strip() if revision.returncode == 0 else None
    try:
        process = subprocess.run(
            [
                str(binary),
                "--root",
                str(repository.path),
                "check",
                *docs,
                "--format",
                "json",
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return ScanResult(
            repository=repository.slug,
            stars=repository.stars,
            commit=commit,
            exit_code=124,
            docs=0,
            tokens=0,
            verified=0,
            broken=0,
            suspect=0,
            silent=0,
            findings=[],
            error=f"attest exceeded the {timeout}s repository timeout",
        )
    if process.returncode not in {0, 1}:
        return ScanResult(
            repository=repository.slug,
            stars=repository.stars,
            commit=commit,
            exit_code=process.returncode,
            docs=0,
            tokens=0,
            verified=0,
            broken=0,
            suspect=0,
            silent=0,
            findings=[],
            error=process.stderr.strip() or "attest failed without an error message",
        )
    try:
        report = json.loads(process.stdout)
        stats = report["stats"]
    except (json.JSONDecodeError, KeyError, TypeError) as error:
        return ScanResult(
            repository=repository.slug,
            stars=repository.stars,
            commit=commit,
            exit_code=2,
            docs=0,
            tokens=0,
            verified=0,
            broken=0,
            suspect=0,
            silent=0,
            findings=[],
            error=f"invalid attest report: {error}",
        )
    return ScanResult(
        repository=repository.slug,
        stars=repository.stars,
        commit=commit,
        exit_code=process.returncode,
        docs=int(stats["docs"]),
        tokens=int(stats["tokens"]),
        verified=int(stats["verified"]),
        broken=int(stats["broken"]),
        suspect=int(stats["suspect"]),
        silent=int(stats["silent"]),
        findings=[
            finding
            for finding in report.get("findings", [])
            if finding.get("verdict") == "broken"
        ],
    )


def report_payload(results: list[ScanResult], profile: str) -> dict[str, object]:
    successful = [result for result in results if result.error is None]
    return {
        "schema": "attest.scan.v1",
        "profile": profile,
        "generated_at": datetime.now(UTC).isoformat(),
        "stats": {
            "repositories": len(results),
            "successful": len(successful),
            "failed": len(results) - len(successful),
            "with_broken": sum(result.broken > 0 for result in successful),
            "docs": sum(result.docs for result in successful),
            "tokens": sum(result.tokens for result in successful),
            "verified": sum(result.verified for result in successful),
            "broken": sum(result.broken for result in successful),
            "suspect": sum(result.suspect for result in successful),
            "silent": sum(result.silent for result in successful),
        },
        "repositories": [asdict(result) for result in results],
    }


def markdown_report(payload: dict[str, object]) -> str:
    stats = payload["stats"]
    assert isinstance(stats, dict)
    successful = int(stats["successful"])
    with_broken = int(stats["with_broken"])
    rate = 100 * with_broken / successful if successful else 0
    rows = [
        "# Public repository scan",
        "",
        f"> Generated {payload['generated_at']} · `attest.scan.v1` · `{payload['profile']}` profile",
        "",
        "## Summary",
        "",
        f"Attest scanned **{successful}** public GitHub repositories and found at least one deterministic broken binding in **{with_broken}** ({rate:.1f}%).",
        "",
        "| Repositories | Docs | Tokens | Verified | Broken | Suspect | Silent |",
        "|---:|---:|---:|---:|---:|---:|---:|",
        f"| {successful} | {stats['docs']} | {stats['tokens']} | {stats['verified']} | {stats['broken']} | {stats['suspect']} | {stats['silent']} |",
        "",
        "## Repositories",
        "",
        "| Repository | Snapshot | Stars | Docs | Broken | Suspect | Status |",
        "|---|---|---:|---:|---:|---:|---|",
    ]
    repositories = payload["repositories"]
    assert isinstance(repositories, list)
    for result in repositories:
        assert isinstance(result, dict)
        status = "error" if result["error"] else "scanned"
        snapshot = str(result["commit"] or "unknown")[:12]
        rows.append(
            f"| [{result['repository']}](https://github.com/{result['repository']}) | `{snapshot}` | {result['stars']} | {result['docs']} | {result['broken']} | {result['suspect']} | {status} |"
        )
    rows.extend(
        [
            "",
            "## Interpretation",
            "",
            "This is an automated static scan. `broken` means a resolver found a deterministic missing binding after context guards; upstream PRs still require manual review against the repository's intended documentation scope. `suspect` never fails CI.",
            "",
        ]
    )
    return "\n".join(rows)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discover", action="append", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/release/attest"))
    parser.add_argument("--limit", type=int, default=50)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--profile", choices=("agent", "all"), default="agent")
    parser.add_argument("--metadata-cache", type=Path)
    parser.add_argument("--json", type=Path, default=Path("reports/public-scan.json"))
    parser.add_argument("--markdown", type=Path, default=Path("reports/public-scan.md"))
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        print(f"attest binary not found: {binary}", file=sys.stderr)
        return 2
    paths = discover_repositories(
        [path.resolve() for path in args.discover], args.profile
    )
    repositories = rank_repositories(
        paths, args.workers, load_cached_stars(args.metadata_cache)
    )
    print(
        f"scanning up to {len(repositories)} public repositories for {args.limit} successful reports",
        file=sys.stderr,
    )
    indexed_results: dict[str, ScanResult] = {}
    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(
                scan_repository, binary, repository, args.timeout, args.profile
            ): repository
            for repository in repositories
        }
        for index, future in enumerate(as_completed(futures), start=1):
            repository = futures[future]
            result = future.result()
            indexed_results[repository.slug] = result
            status = "ok" if result.error is None else "error"
            print(
                f"[{index}/{len(repositories)}] {repository.slug}: {status}",
                file=sys.stderr,
            )
    results = []
    successful = 0
    for repository in repositories:
        result = indexed_results[repository.slug]
        results.append(result)
        successful += result.error is None
        if successful == args.limit:
            break
    payload = report_payload(results, args.profile)
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    args.markdown.write_text(markdown_report(payload), encoding="utf-8")
    return 0 if int(payload["stats"]["successful"]) >= args.limit else 1


if __name__ == "__main__":
    raise SystemExit(main())
