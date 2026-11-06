#!/usr/bin/env python3
"""Validate that a release tag matches package metadata and release notes."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
import tomllib


def validate(root: pathlib.Path, tag: str) -> dict[str, str | bool]:
    if not tag.startswith("v") or len(tag) == 1:
        raise ValueError("release tag must have the form v<package-version>")

    with (root / "Cargo.toml").open("rb") as source:
        manifest = tomllib.load(source)
    version = manifest["package"]["version"]
    expected_tag = f"v{version}"
    if tag != expected_tag:
        raise ValueError(f"tag {tag!r} does not match Cargo version {version!r}")

    with (root / "Cargo.lock").open("rb") as source:
        lockfile = tomllib.load(source)
    root_packages = [
        package
        for package in lockfile["package"]
        if package["name"] == manifest["package"]["name"]
    ]
    if len(root_packages) != 1 or root_packages[0]["version"] != version:
        raise ValueError("Cargo.lock root package version does not match Cargo.toml")

    notes = pathlib.Path("docs") / f"release-notes-{tag}.md"
    notes_path = root / notes
    if not notes_path.is_file() or not notes_path.read_text(encoding="utf-8").strip():
        raise ValueError(f"release notes are missing or empty: {notes}")

    prerelease = "-" in version
    return {
        "tag": tag,
        "version": version,
        "channel": "preview" if prerelease else "stable",
        "prerelease": prerelease,
        "notes_file": notes.as_posix(),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--tag", required=True)
    parser.add_argument("--github-output", type=pathlib.Path)
    arguments = parser.parse_args()

    try:
        result = validate(arguments.root.resolve(), arguments.tag)
    except (KeyError, OSError, TypeError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"release validation failed: {error}", file=sys.stderr)
        return 1

    print(json.dumps(result, sort_keys=True))
    if arguments.github_output:
        with arguments.github_output.open("a", encoding="utf-8") as output:
            for key, value in result.items():
                rendered = str(value).lower() if isinstance(value, bool) else value
                output.write(f"{key}={rendered}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
