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


def prepare(directory: pathlib.Path, output: pathlib.Path) -> dict[str, str]:
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

    destination = output / channel
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    destination.mkdir(parents=True)
    for item in debs:
        name = item.get("name")
        if not isinstance(name, str) or pathlib.Path(name).name != name:
            raise ValueError("manifest contains an unsafe artifact name")
        package = directory / name
        if not package.is_file():
            raise ValueError(f"release asset is missing: {name}")
        if package.stat().st_size != item.get("size") or digest(package) != item.get("sha256"):
            raise ValueError(f"release asset does not match its manifest: {name}")
        shutil.copyfile(package, destination / name)

    return {"tag": tag, "version": version, "suite": channel}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--github-output", type=pathlib.Path)
    arguments = parser.parse_args()
    try:
        metadata = prepare(arguments.directory, arguments.output)
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
