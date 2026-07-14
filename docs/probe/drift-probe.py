#!/usr/bin/env python3
"""Archived feasibility probe, updated with the six fixes recorded in the report.

This intentionally small probe is retained for experiment reproducibility. The Rust
implementation in crates/ is authoritative.
"""

from __future__ import annotations

import argparse
import re
import shutil
from pathlib import Path

INLINE = re.compile(r"`([^`\n]+)`")
FENCE = re.compile(r"```(?:bash|sh|shell|zsh|console)\s*\n(.*?)```", re.DOTALL | re.IGNORECASE)
ENV = re.compile(r"^[A-Z][A-Z0-9_]{2,}$")


def tokens(markdown: str) -> list[str]:
    found = INLINE.findall(markdown)
    for block in FENCE.findall(markdown):
        logical = block.replace("\\\n", " ")
        found.extend(line.strip().removeprefix("$ ") for line in logical.splitlines() if line.strip() and not line.lstrip().startswith("#") and "<<" not in line)
    return found


def bind(root: Path, doc: Path, token: str) -> str:
    first = token.split()[0] if token.split() else ""
    if first and shutil.which(first):
        return "cmd"
    for base in (doc.parent, nearest_project(root, doc.parent), root):
        if (base / token).exists():
            return "path"
    if ENV.fullmatch(token) and grep(root, token):
        return "env"
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_:-]{2,}", token) and grep(root, token):
        return "symbol"
    return "silent"


def nearest_project(root: Path, start: Path) -> Path:
    current = start
    manifests = ("package.json", "Cargo.toml", "go.mod", "pyproject.toml")
    while current != root:
        if any((current / manifest).exists() for manifest in manifests):
            return current
        current = current.parent
    return root


def grep(root: Path, word: str) -> bool:
    for path in root.rglob("*"):
        if path.is_file() and not any(part in {".git", "node_modules", "target"} for part in path.parts):
            try:
                if re.search(rf"(?<![A-Za-z0-9_]){re.escape(word)}(?![A-Za-z0-9_])", path.read_text(errors="ignore")):
                    return True
            except OSError:
                pass
    return False


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("docs", nargs="+", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    counts: dict[str, int] = {}
    for relative in args.docs:
        doc = (root / relative).resolve()
        for token in tokens(doc.read_text()):
            verdict = bind(root, doc, token)
            counts[verdict] = counts.get(verdict, 0) + 1
    print(counts)


if __name__ == "__main__":
    main()
