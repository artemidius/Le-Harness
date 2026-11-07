#!/usr/bin/env python3
"""Select the newest complete APT-capable releases per channel."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys


TAG_PATTERN = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")
DEB_PATTERN = re.compile(r"^le-harness_.+_(amd64|arm64)\.deb$")


def is_eligible(release: dict[str, object]) -> bool:
    tag = release.get("tag_name")
    published = release.get("published_at")
    if release.get("draft") or not isinstance(tag, str) or not TAG_PATTERN.fullmatch(tag):
        return False
    if not isinstance(published, str) or not published:
        return False
    assets = release.get("assets")
    if not isinstance(assets, list):
        return False
    names = {
        asset.get("name")
        for asset in assets
        if isinstance(asset, dict)
    }
    architectures = {
        match.group(1)
        for name in names
        if isinstance(name, str) and (match := DEB_PATTERN.fullmatch(name))
    }
    return "release-manifest.json" in names and architectures == {"amd64", "arm64"}


def select(
    releases: list[dict[str, object]], retention: int, required_tag: str | None = None
) -> list[str]:
    if retention < 1:
        raise ValueError("retention must be positive")
    eligible = [
        release
        for release in releases
        if isinstance(release, dict) and is_eligible(release)
    ]
    eligible.sort(key=lambda release: str(release["published_at"]), reverse=True)

    selected = []
    counts = {"preview": 0, "stable": 0}
    for release in eligible:
        tag = str(release["tag_name"])
        preview = bool(release.get("prerelease"))
        if preview != ("-" in tag.removeprefix("v")):
            raise ValueError(f"release channel disagrees with tag: {tag}")
        channel = "preview" if preview else "stable"
        if counts[channel] < retention:
            selected.append(tag)
            counts[channel] += 1

    if required_tag and required_tag not in selected:
        raise ValueError(
            f"required release is not a complete retained package release: {required_tag}"
        )
    if not selected:
        raise ValueError("no complete package releases are available")
    return selected


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--retention", type=int, default=3)
    parser.add_argument("--required-tag")
    arguments = parser.parse_args()
    try:
        releases = json.loads(arguments.input.read_text(encoding="utf-8"))
        if not isinstance(releases, list):
            raise ValueError("release inventory must be a JSON array")
        tags = select(releases, arguments.retention, arguments.required_tag)
        arguments.output.write_text("\n".join(tags) + "\n", encoding="utf-8")
    except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"APT release selection failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
