#!/usr/bin/env python3
"""Mine likely documentation-drift repairs from local Git repositories."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path

INLINE_CODE = re.compile(r"`([^`\n]+)`")
DOC_NAMES = {"AGENTS.md", "CLAUDE.md", "SKILL.md"}
COMMAND_TOOLS = {
    "aws",
    "az",
    "bash",
    "brew",
    "bun",
    "cargo",
    "claude",
    "cmake",
    "codex",
    "curl",
    "deno",
    "docker",
    "gh",
    "git",
    "gcloud",
    "go",
    "grep",
    "helm",
    "hf",
    "jq",
    "just",
    "kubectl",
    "make",
    "node",
    "npm",
    "npx",
    "pdm",
    "pip",
    "pip3",
    "pnpm",
    "poetry",
    "powershell",
    "pwsh",
    "pytest",
    "python",
    "python3",
    "rg",
    "ruff",
    "scoop",
    "sh",
    "terraform",
    "tofu",
    "uv",
    "wget",
    "wrangler",
    "yarn",
    "zsh",
}
FILE_EXTENSIONS = {
    "bash",
    "c",
    "cc",
    "cfg",
    "cpp",
    "cs",
    "css",
    "csv",
    "go",
    "h",
    "hpp",
    "html",
    "ini",
    "java",
    "js",
    "json",
    "jsx",
    "kt",
    "lock",
    "md",
    "mjs",
    "php",
    "proto",
    "py",
    "rb",
    "rs",
    "scss",
    "sh",
    "sql",
    "swift",
    "toml",
    "ts",
    "tsx",
    "txt",
    "xml",
    "yaml",
    "yml",
    "zsh",
}
CORPUS_CATEGORIES = (
    "path",
    "script",
    "pkg",
    "cmd",
    "go-import",
    "env",
    "config-key",
    "symbol",
    "prose",
)


@dataclass(frozen=True)
class Commit:
    sha: str
    timestamp: int
    subject: str
    files: tuple[str, ...]


@dataclass(frozen=True)
class Case:
    schema: str
    repo: str
    code_commit: str
    doc_commit: str
    doc: str
    before: str
    after: str
    resolver: str
    delay_seconds: int
    code_subject: str
    doc_subject: str
    reviewed: bool


def git(repo: Path, *args: str, check: bool = True) -> str:
    process = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if check and process.returncode != 0:
        raise RuntimeError(process.stderr.strip() or f"git {' '.join(args)} failed")
    return process.stdout


def commits(repo: Path, limit: int | None) -> list[Commit]:
    args = ["log", "--reverse", "--format=@@%H%x09%ct%x09%s", "--name-only"]
    if limit:
        args.insert(1, f"-{limit}")
    output = git(repo, *args)
    found: list[Commit] = []
    sha = ""
    timestamp = 0
    subject = ""
    files: list[str] = []
    for line in output.splitlines() + ["@@END\t0"]:
        if line.startswith("@@"):
            if sha:
                found.append(Commit(sha, timestamp, subject, tuple(files)))
            marker = line[2:].split("\t", 2)
            sha = marker[0]
            timestamp = int(marker[1]) if marker[0] != "END" else 0
            subject = marker[2] if len(marker) > 2 else ""
            files = []
        elif line:
            files.append(line)
    return found


def is_doc(path: str, all_markdown: bool = False) -> bool:
    name = Path(path).name
    return (
        all_markdown and path.endswith(".md")
        or name in DOC_NAMES
        or path.startswith(".claude/") and path.endswith(".md")
    )


def changed_tokens(repo: Path, commit: str, doc: str) -> tuple[list[str], list[str]]:
    diff = git(repo, "diff", f"{commit}^", commit, "--", doc, check=False)
    removed: list[str] = []
    added: list[str] = []
    for line in diff.splitlines():
        if line.startswith("---") or line.startswith("+++"):
            continue
        target = removed if line.startswith("-") else added if line.startswith("+") else None
        if target is not None:
            target.extend(INLINE_CODE.findall(line[1:]))
    return unique(removed), unique(added)


def unique(values: list[str]) -> list[str]:
    return list(dict.fromkeys(value.strip() for value in values if value.strip()))


def classify(token: str) -> str:
    if re.fullmatch(r"[A-Z][A-Z0-9_]{2,}", token):
        return "env"
    if re.search(r"\b(?:npm|pnpm|yarn|bun)\s+run\s+\S+", token):
        return "script"
    first_word = token.removeprefix("$ ").split(maxsplit=1)[0].lower()
    if first_word in COMMAND_TOOLS and len(token.split()) > 1:
        return "cmd"
    if re.fullmatch(r"@[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", token):
        return "pkg"
    if re.fullmatch(
        r"(?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,}/[A-Za-z0-9_./-]+", token
    ):
        return "go-import"
    suffix = Path(token).suffix.removeprefix(".").lower()
    if "/" in token or suffix in FILE_EXTENSIONS:
        return "path"
    if re.fullmatch(r"[a-z][a-z0-9_-]*(?:\.[a-z][a-z0-9_-]*)+", token):
        return "symbol"
    if re.search(r"\.[A-Za-z0-9]{1,8}$", token):
        return "path"
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_:-]+", token):
        return "symbol"
    return "prose"


def pair_tokens(before: list[str], after: list[str], threshold: float) -> list[tuple[str, str]]:
    if not before or not after:
        return []
    pairs: list[tuple[str, str]] = []
    unused = set(range(len(after)))
    for old in before:
        candidates = [(similarity(old, after[index]), index) for index in unused]
        if not candidates:
            break
        score, index = max(candidates)
        if score >= threshold and old != after[index]:
            pairs.append((old, after[index]))
            unused.remove(index)
    return pairs


def similarity(left: str, right: str) -> float:
    left_parts = set(re.split(r"[/:._\s-]+", left.lower()))
    right_parts = set(re.split(r"[/:._\s-]+", right.lower()))
    union = left_parts | right_parts
    return len(left_parts & right_parts) / len(union) if union else 0.0


def path_candidates(doc: str, token: str) -> list[str]:
    if (
        not token
        or any(character in token for character in "<>{}$*?")
        or any(character.isspace() for character in token)
        or token.startswith(("/", "http://", "https://"))
    ):
        return []
    token = token.removeprefix("./").rstrip("/.,:;")
    if (
        not token
        or token.startswith("@")
        or "/" not in token
        and Path(token).suffix == ""
    ):
        return []
    doc_dir = str(Path(doc).parent)
    candidates = [token]
    if doc_dir != ".":
        candidates.insert(0, str(Path(doc_dir) / token))
    return list(dict.fromkeys(candidates))


def git_path_exists(repo: Path, commit: str, path: str) -> bool:
    process = subprocess.run(
        ["git", "cat-file", "-e", f"{commit}:{path}"],
        cwd=repo,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return process.returncode == 0


def verify_path_transition(repo: Path, commit: str, doc: str, old: str, new: str) -> tuple[bool, str | None]:
    old_paths = path_candidates(doc, old)
    new_paths = path_candidates(doc, new)
    if not old_paths or not new_paths:
        return False, None
    old_existed = any(git_path_exists(repo, f"{commit}^", path) for path in old_paths)
    existing_new = next((path for path in new_paths if git_path_exists(repo, commit, path)), None)
    return not old_existed and existing_new is not None, existing_new


def path_change_commit(repo: Path, before_commit: str, path: str) -> Commit | None:
    output = git(
        repo,
        "log",
        "-1",
        "--format=%H%x09%ct%x09%s",
        f"{before_commit}^",
        "--",
        path,
        check=False,
    ).strip()
    if not output:
        return None
    sha, timestamp, subject = output.split("\t", 2)
    return Commit(sha, int(timestamp), subject, (path,))


def mine(repo: Path, limit: int | None, threshold: float, all_markdown: bool) -> list[Case]:
    history = commits(repo, limit)
    repo_name = git(repo, "remote", "get-url", "origin", check=False).strip() or repo.name
    previous_code: Commit | None = None
    cases: list[Case] = []
    for commit in history:
        docs = [path for path in commit.files if is_doc(path, all_markdown)]
        code_changed = any(not is_doc(path, all_markdown) for path in commit.files)
        if docs and previous_code and not code_changed:
            delay = commit.timestamp - previous_code.timestamp
            if delay < 0 or delay > 180 * 24 * 60 * 60:
                continue
            for doc in docs:
                before, after = changed_tokens(repo, commit.sha, doc)
                for old, new in pair_tokens(before, after, threshold):
                    resolver = classify(old)
                    reviewed, changed_path = (
                        verify_path_transition(repo, commit.sha, doc, old, new)
                        if resolver == "path"
                        else (False, None)
                    )
                    source = (
                        path_change_commit(repo, commit.sha, changed_path)
                        if reviewed and changed_path
                        else None
                    ) or previous_code
                    cases.append(
                        Case(
                            schema="attest.corpus.v1",
                            repo=repo_name,
                            code_commit=source.sha,
                            doc_commit=commit.sha,
                            doc=doc,
                            before=old,
                            after=new,
                            resolver=resolver,
                            delay_seconds=delay,
                            code_subject=source.subject,
                            doc_subject=commit.subject,
                            reviewed=reviewed,
                        )
                    )
        if code_changed:
            previous_code = commit
    return cases


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repos", nargs="*", type=Path, help="local Git repositories")
    parser.add_argument(
        "--discover",
        action="append",
        type=Path,
        default=[],
        help="recursively discover Git repositories below this directory",
    )
    parser.add_argument("--output", type=Path, help="write JSONL here (default: stdout)")
    parser.add_argument("--summary", type=Path, help="write a deterministic JSON summary here")
    parser.add_argument("--limit", type=int, help="maximum commits per repository")
    parser.add_argument(
        "--threshold",
        type=float,
        default=0.55,
        help="minimum token similarity from 0 to 1 (default: 0.55)",
    )
    parser.add_argument(
        "--all-markdown",
        action="store_true",
        help="include every Markdown file, not only agent instruction documents",
    )
    parser.add_argument(
        "--reviewed-only",
        action="store_true",
        help="write only snapshot-verified cases",
    )
    parser.add_argument(
        "--github-only",
        action="store_true",
        help="write only cases whose origin remote is hosted on github.com",
    )
    return parser.parse_args()


def summary_payload(cases: list[Case], output: str) -> dict[str, object]:
    resolver_counts = Counter(case.resolver for case in cases)
    repository_counts: dict[str, Counter[str]] = defaultdict(Counter)
    for case in cases:
        repository_counts[case.repo][case.resolver] += 1
    return {
        "schema": "attest.corpus-summary.v1",
        "candidate_sha256": hashlib.sha256(output.encode()).hexdigest(),
        "stats": {
            "cases": len(cases),
            "reviewed": sum(case.reviewed for case in cases),
            "repositories": len({case.repo for case in cases}),
            "documents": len({(case.repo, case.doc) for case in cases}),
            "doc_commits": len({case.doc_commit for case in cases}),
        },
        "resolvers": {
            category: resolver_counts[category] for category in CORPUS_CATEGORIES
        },
        "repositories": {
            repository: dict(sorted(counts.items()))
            for repository, counts in sorted(repository_counts.items())
        },
    }


def main() -> int:
    args = parse_args()
    repos = list(args.repos)
    for directory in args.discover:
        repos.extend(path.parent for path in directory.rglob(".git") if path.is_dir())
    repos = sorted(set(repo.resolve() for repo in repos))
    if not repos:
        print("no repositories supplied or discovered", file=sys.stderr)
        return 2
    cases: list[Case] = []
    failures = 0
    for repo in repos:
        try:
            cases.extend(mine(repo.resolve(), args.limit, args.threshold, args.all_markdown))
        except (OSError, RuntimeError) as error:
            print(f"{repo}: {error}", file=sys.stderr)
            failures += 1
    output_cases = [case for case in cases if case.reviewed] if args.reviewed_only else cases
    if args.github_only:
        output_cases = [case for case in output_cases if "github.com" in case.repo]
    output = "".join(json.dumps(asdict(case), ensure_ascii=False) + "\n" for case in output_cases)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output, encoding="utf-8")
    else:
        sys.stdout.write(output)
    if args.summary:
        args.summary.parent.mkdir(parents=True, exist_ok=True)
        args.summary.write_text(
            json.dumps(summary_payload(output_cases, output), indent=2) + "\n",
            encoding="utf-8",
        )
    print(
        f"mined {len(cases)} cases, wrote {len(output_cases)} "
        f"({failures} repositories skipped)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
