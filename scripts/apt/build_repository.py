#!/usr/bin/env python3
"""Build and sign a static APT repository snapshot."""

from __future__ import annotations

import argparse
import datetime as dt
import email.utils
import gzip
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time


SUITE_PATTERN = re.compile(r"^[a-z0-9][a-z0-9.-]*$")
PACKAGE_PATTERN = re.compile(r"^le-harness_.*\.deb$")


def run(*arguments: str, cwd: pathlib.Path | None = None) -> str:
    result = subprocess.run(
        arguments,
        cwd=cwd,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return result.stdout


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def parse_suite(value: str) -> tuple[str, pathlib.Path]:
    suite, separator, directory = value.partition("=")
    if not separator or not SUITE_PATTERN.fullmatch(suite) or not directory:
        raise argparse.ArgumentTypeError("suite must have the form name=/package/directory")
    return suite, pathlib.Path(directory).resolve()


def package_field(package: pathlib.Path, field: str) -> str:
    return run("dpkg-deb", "--field", str(package), field).strip()


def repository_size(root: pathlib.Path) -> int:
    return sum(path.stat().st_size for path in root.rglob("*") if path.is_file())


def write_indexes(
    root: pathlib.Path, suite: str, package_directory: pathlib.Path, architectures: list[str]
) -> list[dict[str, str | int]]:
    pool = root / "pool" / suite / "main" / "l" / "le-harness"
    pool.mkdir(parents=True)
    inventory: list[dict[str, str | int]] = []

    packages = sorted(path for path in package_directory.iterdir() if PACKAGE_PATTERN.match(path.name))
    if not packages:
        raise ValueError(f"suite {suite!r} contains no le-harness Debian packages")
    seen_architectures = set()
    for package in packages:
        if package_field(package, "Package") != "le-harness":
            raise ValueError(f"unexpected package name in {package}")
        architecture = package_field(package, "Architecture")
        if architecture not in architectures:
            raise ValueError(f"unsupported architecture {architecture!r} in {package}")
        destination = pool / package.name
        if destination.exists() and sha256(destination) != sha256(package):
            raise ValueError(f"package path collision for {package.name}")
        shutil.copyfile(package, destination)
        seen_architectures.add(architecture)
        inventory.append(
            {
                "suite": suite,
                "name": package.name,
                "version": package_field(package, "Version"),
                "architecture": architecture,
                "size": destination.stat().st_size,
                "sha256": sha256(destination),
            }
        )

    missing = sorted(set(architectures) - seen_architectures)
    if missing:
        raise ValueError(f"suite {suite!r} is missing architectures: {', '.join(missing)}")

    for architecture in architectures:
        binary = root / "dists" / suite / "main" / f"binary-{architecture}"
        binary.mkdir(parents=True)
        relative_pool = pool.relative_to(root)
        packages_text = run(
            "dpkg-scanpackages",
            "--arch",
            architecture,
            str(relative_pool),
            "/dev/null",
            cwd=root,
        ).encode()
        packages_path = binary / "Packages"
        packages_path.write_bytes(packages_text)
        (binary / "Packages.gz").write_bytes(gzip.compress(packages_text, mtime=0))

        for index in [packages_path, binary / "Packages.gz"]:
            by_hash = binary / "by-hash" / "SHA256" / sha256(index)
            by_hash.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(index, by_hash)

    return inventory


def write_release(
    root: pathlib.Path,
    suite: str,
    architectures: list[str],
    generated: dt.datetime,
    valid_until: dt.datetime,
) -> pathlib.Path:
    suite_root = root / "dists" / suite
    options = {
        "Origin": "Le Harness",
        "Label": "Le Harness",
        "Suite": suite,
        "Codename": suite,
        "Architectures": " ".join(architectures),
        "Components": "main",
        "Description": f"Le Harness {suite} packages",
        "Date": email.utils.format_datetime(generated),
        "Valid-Until": email.utils.format_datetime(valid_until),
        "Acquire-By-Hash": "yes",
    }
    command = ["apt-ftparchive"]
    for key, value in options.items():
        command.extend(["-o", f"APT::FTPArchive::Release::{key}={value}"])
    command.extend(["release", str(suite_root.relative_to(root))])
    release = suite_root / "Release"
    content = run(*command, cwd=root)
    if not any(line.startswith("Valid-Until:") for line in content.splitlines()):
        content = f"Valid-Until: {options['Valid-Until']}\n{content}"
    release.write_text(content, encoding="utf-8")
    return release


def sign_release(
    release: pathlib.Path,
    signing_key: str,
    passphrase_file: pathlib.Path | None = None,
) -> None:
    run("gpg", "--batch", "--list-secret-keys", signing_key)
    passphrase_arguments = []
    if passphrase_file is not None:
        passphrase_arguments = [
            "--pinentry-mode",
            "loopback",
            "--passphrase-file",
            str(passphrase_file),
        ]
    run(
        "gpg",
        "--batch",
        "--yes",
        *passphrase_arguments,
        "--local-user",
        signing_key,
        "--digest-algo",
        "SHA256",
        "--clearsign",
        "--output",
        str(release.parent / "InRelease"),
        str(release),
    )
    run(
        "gpg",
        "--batch",
        "--yes",
        *passphrase_arguments,
        "--local-user",
        signing_key,
        "--digest-algo",
        "SHA256",
        "--detach-sign",
        "--output",
        str(release.parent / "Release.gpg"),
        str(release),
    )


def signing_fingerprint(signing_key: str) -> str:
    output = run("gpg", "--batch", "--with-colons", "--fingerprint", signing_key)
    fingerprints = [line.split(":")[9] for line in output.splitlines() if line.startswith("fpr:")]
    if not fingerprints:
        raise ValueError("signing key has no fingerprint")
    return fingerprints[0]


def build(arguments: argparse.Namespace) -> None:
    output = arguments.output.resolve()
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"output directory must be absent or empty: {output}")
    output.mkdir(parents=True, exist_ok=True)

    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", str(int(time.time()))))
    generated = dt.datetime.fromtimestamp(epoch, tz=dt.timezone.utc)
    valid_until = generated + dt.timedelta(days=arguments.valid_days)
    architectures = [value.strip() for value in arguments.architectures.split(",")]
    if not architectures or any(architecture not in {"amd64", "arm64"} for architecture in architectures):
        raise ValueError("architectures must be a comma-separated subset of amd64,arm64")

    suites = [suite for suite, _ in arguments.suite]
    if len(suites) != len(set(suites)):
        raise ValueError("suite names must be unique")

    inventory = []
    for suite, package_directory in arguments.suite:
        if not package_directory.is_dir():
            raise ValueError(f"package directory does not exist: {package_directory}")
        inventory.extend(write_indexes(output, suite, package_directory, architectures))
        sign_release(
            write_release(output, suite, architectures, generated, valid_until),
            arguments.signing_key,
            arguments.passphrase_file,
        )

    fingerprint = signing_fingerprint(arguments.signing_key)
    with (output / "le-harness-archive-keyring.gpg").open("wb") as public_key:
        subprocess.run(
            ["gpg", "--batch", "--export", fingerprint], check=True, stdout=public_key
        )
    with (output / "le-harness-archive-keyring.asc").open("wb") as armored_key:
        subprocess.run(
            ["gpg", "--batch", "--armor", "--export", fingerprint],
            check=True,
            stdout=armored_key,
        )
    (output / "SIGNING-KEY-FINGERPRINT").write_text(f"{fingerprint}\n", encoding="ascii")

    metadata = {
        "schema_version": 1,
        "generated_at": generated.isoformat(),
        "valid_until": valid_until.isoformat(),
        "signing_key_fingerprint": fingerprint,
        "architectures": architectures,
        "suites": suites,
        "repository_bytes": 0,
        "packages": inventory,
    }
    manifest = output / "repository-manifest.json"
    for _ in range(3):
        manifest.write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        metadata["repository_bytes"] = repository_size(output)
    manifest.write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    size = repository_size(output)
    if size > arguments.max_bytes:
        raise ValueError(f"repository size {size} exceeds limit {arguments.max_bytes}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--suite", action="append", type=parse_suite, required=True)
    parser.add_argument("--architectures", default="amd64,arm64")
    parser.add_argument("--signing-key", required=True)
    parser.add_argument("--passphrase-file", type=pathlib.Path)
    parser.add_argument("--valid-days", type=int, default=30)
    parser.add_argument("--max-bytes", type=int, default=200 * 1024 * 1024)
    arguments = parser.parse_args()

    if arguments.passphrase_file is not None:
        arguments.passphrase_file = arguments.passphrase_file.resolve()
        if not arguments.passphrase_file.is_file():
            parser.error("--passphrase-file must name an existing file")
    if arguments.valid_days < 1:
        parser.error("--valid-days must be positive")

    try:
        build(arguments)
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(f"APT repository build failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
