#!/usr/bin/env python3
"""Validate released Debian assets before repository publication."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import shutil
import sys


ARCHITECTURES = {"amd64", "arm64"}


def digest(path: pathlib.Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def validate(directory: pathlib.Path) -> tuple[dict[str, str], list[pathlib.Path]]:
    manifest_path = directory / "release-manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1:
        raise ValueError("unsupported release manifest schema")

    channel = manifest.get("channel")
    version = manifest.get("version")
    tag = manifest.get("tag")
    if channel not in {"preview", "stable"}:
        raise ValueError("release channel must be preview or stable")
    if not isinstance(version, str) or tag != f"v{version}":
        raise ValueError("release tag and version do not agree")
    if ("-" in version) != (channel == "preview"):
        raise ValueError("release version does not agree with its channel")

    debs = [item for item in manifest.get("artifacts", []) if item.get("kind") == "deb"]
    architectures = {item.get("architecture") for item in debs}
    if architectures != ARCHITECTURES or len(debs) != len(ARCHITECTURES):
        raise ValueError("release must contain one Debian package per architecture")

    packages = []
    for item in debs:
        name = item.get("name")
        if not isinstance(name, str) or pathlib.Path(name).name != name:
            raise ValueError("manifest contains an unsafe artifact name")
        package = directory / name
        if not package.is_file():
            raise ValueError(f"release asset is missing: {name}")
        if package.stat().st_size != item.get("size") or digest(package) != item.get("sha256"):
            raise ValueError(f"release asset does not match its manifest: {name}")
        packages.append(package)

    return {"tag": tag, "version": version, "suite": channel}, packages


def prepare_many(
    directories: list[pathlib.Path], output: pathlib.Path, retention: int = 3
) -> dict[str, str]:
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    if retention < 1:
        raise ValueError("retention must be positive")
    if not directories:
        raise ValueError("at least one release directory is required")

    releases = [validate(directory) for directory in directories]
    tags = [metadata["tag"] for metadata, _ in releases]
    if len(tags) != len(set(tags)):
        raise ValueError("release inventory contains duplicate tags")

    counts: dict[str, int] = {}
    for metadata, _ in releases:
        suite = metadata["suite"]
        counts[suite] = counts.get(suite, 0) + 1
        if counts[suite] > retention:
            raise ValueError(f"suite {suite!r} exceeds retention limit {retention}")

    for metadata, packages in releases:
        destination = output / metadata["suite"]
        destination.mkdir(parents=True, exist_ok=True)
        for package in packages:
            target = destination / package.name
            if target.exists():
                raise ValueError(f"duplicate package filename: {package.name}")
            shutil.copyfile(package, target)

    return {
        "suites": ",".join(sorted(counts)),
        "tags": ",".join(tags),
    }


def prepare(directory: pathlib.Path, output: pathlib.Path) -> dict[str, str]:
    metadata, _ = validate(directory)
    prepare_many([directory], output)
    return metadata


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--directory", action="append", type=pathlib.Path, required=True
    )
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--retention", type=int, default=3)
    parser.add_argument("--github-output", type=pathlib.Path)
    arguments = parser.parse_args()
    try:
        metadata = prepare_many(
            arguments.directory, arguments.output, arguments.retention
        )
        if arguments.github_output:
            with arguments.github_output.open("a", encoding="utf-8") as destination:
                for key, value in metadata.items():
                    destination.write(f"{key}={value}\n")
    except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"APT snapshot preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
