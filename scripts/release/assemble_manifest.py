#!/usr/bin/env python3
"""Create deterministic checksums and a machine-readable release manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys


TARGETS = {
    "x86_64-unknown-linux-gnu": "tar.gz",
    "aarch64-unknown-linux-gnu": "tar.gz",
    "aarch64-apple-darwin": "tar.gz",
    "x86_64-pc-windows-msvc": "zip",
}

DEBIAN_ARCHITECTURES = {
    "x86_64-unknown-linux-gnu": "amd64",
    "aarch64-unknown-linux-gnu": "arm64",
}


def digest(path: pathlib.Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def debian_version(version: str, revision: str = "1") -> str:
    upstream, separator, prerelease = version.partition("-")
    normalized = f"{upstream}~{prerelease}" if separator else upstream
    return f"{normalized}-{revision}"


def assemble(directory: pathlib.Path, tag: str, commit: str) -> dict[str, object]:
    version = tag.removeprefix("v")
    if not tag.startswith("v") or not version:
        raise ValueError("release tag must have the form v<package-version>")
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        raise ValueError("source commit must be a 40-character lowercase hexadecimal SHA")

    artifacts = []
    for target, extension in TARGETS.items():
        name = f"le-harness-{tag}-{target}.{extension}"
        path = directory / name
        if not path.is_file():
            raise ValueError(f"required artifact is missing: {name}")
        artifacts.append(
            {
                "kind": "archive",
                "name": name,
                "target": target,
                "size": path.stat().st_size,
                "sha256": digest(path),
            }
        )

    for target, architecture in DEBIAN_ARCHITECTURES.items():
        name = f"le-harness_{debian_version(version)}_{architecture}.deb"
        path = directory / name
        if not path.is_file():
            raise ValueError(f"required artifact is missing: {name}")
        artifacts.append(
            {
                "kind": "deb",
                "name": name,
                "target": target,
                "architecture": architecture,
                "size": path.stat().st_size,
                "sha256": digest(path),
            }
        )

    unexpected = sorted(
        path.name
        for path in directory.glob("le-harness*")
        if path.is_file() and path.name not in {item["name"] for item in artifacts}
    )
    if unexpected:
        raise ValueError(f"unexpected release artifacts: {', '.join(unexpected)}")

    return {
        "schema_version": 1,
        "tag": tag,
        "version": version,
        "channel": "preview" if "-" in version else "stable",
        "source_commit": commit,
        "artifacts": artifacts,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=pathlib.Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    arguments = parser.parse_args()

    try:
        result = assemble(arguments.directory, arguments.tag, arguments.commit)
        manifest_path = arguments.directory / "release-manifest.json"
        manifest_path.write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        checksum_lines = [
            f"{artifact['sha256']}  {artifact['name']}"
            for artifact in result["artifacts"]
        ]
        (arguments.directory / "SHA256SUMS").write_text(
            "\n".join(checksum_lines) + "\n", encoding="utf-8"
        )
    except (OSError, TypeError, ValueError) as error:
        print(f"manifest assembly failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
