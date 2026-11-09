# Release process

Le Harness releases are assembled from one tagged commit. The tag, package
version, lockfile root package, and versioned release-notes file must agree
before any build starts.

## Artifact contract

The release workflow builds these archives and Debian packages:

| Target | Build environment | Archive |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Native x86_64, Debian 12 container | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Native ARM64, Debian 12 container | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 ARM64 runner | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 runner | `.zip` |
| Debian amd64 | Same Debian 12 x86_64 release binary | `.deb` |
| Debian arm64 | Same Debian 12 ARM64 release binary | `.deb` |

Linux artifacts use native runners rather than emulation. The pinned Debian 12
userspace is the compatibility baseline; package compatibility is still tested
separately on every advertised distribution.

Each build runs the packaged binary without provider credentials. Linux builds
also record `file` and `ldd` output as workflow evidence. The public bundle
contains the archives, Debian packages, `SHA256SUMS`, and
`release-manifest.json`. The manifest
records the exact tag, version, channel, source commit, target, byte size, and
SHA-256 digest.

## Publication behavior

1. Create the versioned notes file `docs/release-notes-v<version>.md`.
2. Update `Cargo.toml` and `Cargo.lock` to the same version.
3. Complete the release checklist and merge all intended changes.
4. Create and push the matching `v<version>` tag.
5. Let the workflow build and verify the complete inventory.
6. Inspect the draft release and its workflow evidence before publishing it.

The publication job is the only job with repository write permission. It
creates a draft and never silently replaces an existing asset. On rerun, every
existing asset is downloaded and compared byte-for-byte; mismatched bytes fail
the release. Concurrency is serialized per tag.

The optional manual workflow input accepts an existing tag. It is for verifying
or completing an idempotent staged release, not for building an untagged branch.
Stable versions omit the prerelease flag; versions containing a SemVer
prerelease suffix are marked as preview releases.

Publishing a release also runs the Homebrew update workflow. It downloads the
three advertised Homebrew archives and `release-manifest.json`, verifies every
size and SHA-256 digest, and deterministically renders the tap formula. A
repository-scoped deploy key can push only to `artemidius/homebrew-tap`; it
creates an `automation/le-harness-v<version>` branch for the tap's own CI and
pull-request workflow. Duplicate invocations are no-ops, and version ordering
prevents an older or preview release from replacing a newer stable formula.
The tap workflow refuses to open a generated PR if that branch changes any
path except `Formula/le-harness.rb`.

The deploy key is named `Le Harness release automation` in the tap and its
private half is stored only as the `HOMEBREW_TAP_DEPLOY_KEY` Actions secret in
Le Harness plus operator-controlled credential storage. To rotate it, add a
replacement tap deploy key, replace the secret, run this workflow manually for
the current published tag, and remove the old key only after the no-op run
succeeds.

## Local validation

```text
python3 scripts/release/validate_release.py --tag v0.1.0-rc.7
python3 -m unittest discover -s tests/release -p 'test_*.py'
```

The manifest assembler is normally run after all four build artifacts have
been downloaded into one directory. It rejects missing or unexpected
`le-harness-*` files.
