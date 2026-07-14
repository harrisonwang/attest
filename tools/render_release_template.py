#!/usr/bin/env python3
import argparse
import os
import re
import sys
from pathlib import Path


SIMPLE_TOKEN_RE = re.compile(r"\{\{([A-Z][A-Z0-9_]*)\}\}")
SHA_TOKEN_RE = re.compile(r"\{\{SHA256:([^}]+)\}\}")


def class_name_for(name: str) -> str:
    return "".join(part[:1].upper() + part[1:] for part in name.split("-") if part)


def load_checksums(path: Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) >= 2:
            checksums[Path(parts[-1].lstrip("*")).name] = parts[0]
    return checksums


def render(template: str, variables: dict[str, str], checksums: dict[str, str]) -> str:
    def simple_token(match: re.Match[str]) -> str:
        key = match.group(1)
        if key not in variables:
            raise KeyError(f"missing template variable: {key}")
        return variables[key]

    def sha_token(match: re.Match[str]) -> str:
        asset = match.group(1)
        if asset not in checksums:
            raise KeyError(f"missing checksum for asset: {asset}")
        return checksums[asset]

    rendered = SIMPLE_TOKEN_RE.sub(simple_token, template)
    rendered = SHA_TOKEN_RE.sub(sha_token, rendered)
    if "{{" in rendered or "}}" in rendered:
        raise ValueError("unresolved template token remains")
    return rendered


def main() -> int:
    parser = argparse.ArgumentParser(description="Render a release channel template.")
    parser.add_argument("--template", required=True, type=Path)
    parser.add_argument("--checksums", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    tool = os.environ.get("TOOL", "")
    tag = os.environ.get("TAG", "")
    repo = os.environ.get("REPO", "")
    variables = {
        key: value
        for key, value in os.environ.items()
        if re.fullmatch(r"[A-Z][A-Z0-9_]*", key) and value
    }
    if tool:
        variables.setdefault("CLASS_NAME", class_name_for(tool))
    if tag:
        variables.setdefault("VERSION", tag.removeprefix("v"))
    if repo and tag:
        variables.setdefault("BASE_URL", f"https://github.com/{repo}/releases/download/{tag}")

    try:
        rendered = render(
            args.template.read_text(encoding="utf-8"),
            variables,
            load_checksums(args.checksums),
        )
    except Exception as error:
        print(f"render_release_template.py: {error}", file=sys.stderr)
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
