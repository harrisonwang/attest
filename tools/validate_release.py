#!/usr/bin/env python3
"""Validate source-controlled release metadata before building or publishing."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path


SEMVER_RE = re.compile(
    r"^(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)
PACKAGE_NAMES = {"attest-cli", "attest-core"}


def validate_release(root: Path, tag: str | None = None) -> list[str]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    package = json.loads(
        (root / "packages/npm/package.json").read_text(encoding="utf-8")
    )
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))

    version = workspace["workspace"]["package"]["version"]
    errors: list[str] = []
    if not SEMVER_RE.fullmatch(version):
        errors.append(f"workspace version is not valid SemVer: {version}")
    if package.get("version") != version:
        errors.append(
            "npm package version does not match workspace version: "
            f"{package.get('version')} != {version}"
        )

    lock_versions = {
        entry["name"]: entry["version"]
        for entry in lock.get("package", [])
        if entry.get("name") in PACKAGE_NAMES
    }
    if set(lock_versions) != PACKAGE_NAMES:
        errors.append("Cargo.lock does not contain both attest workspace packages")
    for name in sorted(PACKAGE_NAMES):
        if lock_versions.get(name) != version:
            errors.append(
                f"Cargo.lock version for {name} does not match workspace: "
                f"{lock_versions.get(name)} != {version}"
            )

    repository = workspace["workspace"]["package"]["repository"]
    expected_npm_repository = f"git+{repository}.git"
    actual_npm_repository = package.get("repository", {}).get("url")
    if actual_npm_repository != expected_npm_repository:
        errors.append(
            "npm repository URL does not match the Cargo workspace repository: "
            f"{actual_npm_repository} != {expected_npm_repository}"
        )

    if package.get("name") != "@harrisonwang/attest":
        errors.append("unexpected npm package name")
    if package.get("publishConfig", {}).get("access") != "public":
        errors.append("npm publishConfig.access must be public")
    if tag is not None and tag != f"v{version}":
        errors.append(f"release tag {tag} does not match source version v{version}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--tag")
    args = parser.parse_args()

    try:
        errors = validate_release(args.root.resolve(), args.tag)
    except (OSError, KeyError, TypeError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"validate_release.py: unable to read release metadata: {error}", file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(f"validate_release.py: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
