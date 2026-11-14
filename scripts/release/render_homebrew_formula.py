#!/usr/bin/env python3
"""Render the Homebrew formula from a verified release manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys
from typing import NamedTuple


TARGETS = (
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)
HEX_DIGEST = re.compile(r"[0-9a-f]{64}")
VERSION_FROM_FORMULA = re.compile(
    r"releases/download/v([^/]+)/le-harness-v\1-[^\"]+\.tar\.gz"
)


class Version(NamedTuple):
    core: tuple[int, int, int]
    prerelease: tuple[int | str, ...] | None

    @classmethod
    def parse(cls, value: str) -> "Version":
        match = re.fullmatch(
            r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
            r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?",
            value,
        )
        if not match:
            raise ValueError(f"unsupported release version: {value!r}")
        prerelease = match.group(4)
        identifiers = None
        if prerelease is not None:
            identifiers = tuple(
                int(identifier) if identifier.isdigit() else identifier
                for identifier in prerelease.split(".")
            )
        return cls(tuple(int(match.group(index)) for index in range(1, 4)), identifiers)

    def __lt__(self, other: "Version") -> bool:
        if self.core != other.core:
            return self.core < other.core
        if self.prerelease is None:
            return False
        if other.prerelease is None:
            return True
        for left, right in zip(self.prerelease, other.prerelease):
            if left == right:
                continue
            if isinstance(left, int) and isinstance(right, str):
                return True
            if isinstance(left, str) and isinstance(right, int):
                return False
            return left < right
        return len(self.prerelease) < len(other.prerelease)


def digest(path: pathlib.Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def load_archives(
    manifest_path: pathlib.Path, assets_directory: pathlib.Path
) -> tuple[str, dict[str, dict[str, object]]]:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1:
        raise ValueError("unsupported release manifest schema")

    version = manifest.get("version")
    tag = manifest.get("tag")
    channel = manifest.get("channel")
    if not isinstance(version, str) or tag != f"v{version}":
        raise ValueError("release manifest tag and version do not agree")
    parsed_version = Version.parse(version)
    expected_channel = "preview" if parsed_version.prerelease else "stable"
    if channel != expected_channel:
        raise ValueError("release manifest channel does not match its version")

    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list):
        raise ValueError("release manifest artifacts must be a list")

    archives: dict[str, dict[str, object]] = {}
    for artifact in artifacts:
        if not isinstance(artifact, dict) or artifact.get("kind") != "archive":
            continue
        target = artifact.get("target")
        if target not in TARGETS:
            continue
        if target in archives:
            raise ValueError(f"duplicate archive entry for {target}")
        expected_name = f"le-harness-v{version}-{target}.tar.gz"
        if artifact.get("name") != expected_name:
            raise ValueError(f"unexpected archive name for {target}")
        checksum = artifact.get("sha256")
        if not isinstance(checksum, str) or not HEX_DIGEST.fullmatch(checksum):
            raise ValueError(f"invalid SHA-256 digest for {target}")

        asset = assets_directory / expected_name
        if not asset.is_file():
            raise ValueError(f"required Homebrew asset is missing: {expected_name}")
        if artifact.get("size") != asset.stat().st_size:
            raise ValueError(f"release asset size mismatch: {expected_name}")
        if digest(asset) != checksum:
            raise ValueError(f"release asset checksum mismatch: {expected_name}")
        archives[target] = artifact

    missing = sorted(set(TARGETS) - archives.keys())
    if missing:
        raise ValueError(f"release manifest is missing Homebrew targets: {', '.join(missing)}")
    return version, archives


def current_version(formula: pathlib.Path) -> str:
    source = formula.read_text(encoding="utf-8")
    versions = set(VERSION_FROM_FORMULA.findall(source))
    if len(versions) != 1:
        raise ValueError("could not determine one current version from the formula")
    return versions.pop()


def render(version: str, archives: dict[str, dict[str, object]]) -> str:
    def source(target: str) -> tuple[str, str]:
        artifact = archives[target]
        url = (
            "https://github.com/artemidius/Le-Harness/releases/download/"
            f"v{version}/{artifact['name']}"
        )
        return url, str(artifact["sha256"])

    mac_url, mac_sha = source("aarch64-apple-darwin")
    arm_url, arm_sha = source("aarch64-unknown-linux-gnu")
    intel_url, intel_sha = source("x86_64-unknown-linux-gnu")
    return f'''class LeHarness < Formula
  desc "Transparent coding-agent runtime for the Mistral ecosystem"
  homepage "https://github.com/artemidius/Le-Harness"
  license "MIT"

  depends_on "ca-certificates"
  depends_on "git"

  on_macos do
    depends_on arch: :arm64

    on_arm do
      url "{mac_url}"
      sha256 "{mac_sha}"
    end
  end

  on_linux do
    on_arm do
      url "{arm_url}"
      sha256 "{arm_sha}"
    end

    on_intel do
      url "{intel_url}"
      sha256 "{intel_sha}"
    end
  end

  def install
    bin.install "le-harness"
  end

  test do
    assert_equal "le-harness #{{version}}", shell_output("#{{bin}}/le-harness --version").strip
    output = shell_output("#{{bin}}/le-harness --no-input doctor")
    assert_match "MISTRAL_API_KEY environment override: missing", output
    assert_match "Docker isolation:", output
  end
end
'''


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=pathlib.Path, required=True)
    parser.add_argument("--assets-directory", type=pathlib.Path, required=True)
    parser.add_argument("--current-formula", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    arguments = parser.parse_args()

    try:
        version, archives = load_archives(
            arguments.manifest.resolve(), arguments.assets_directory.resolve()
        )
        installed = current_version(arguments.current_formula.resolve())
        if Version.parse(version) < Version.parse(installed):
            raise ValueError(
                f"refusing to replace newer Homebrew version {installed} with {version}"
            )
        arguments.output.write_text(render(version, archives), encoding="utf-8")
    except (json.JSONDecodeError, OSError, TypeError, ValueError) as error:
        print(f"Homebrew formula generation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
