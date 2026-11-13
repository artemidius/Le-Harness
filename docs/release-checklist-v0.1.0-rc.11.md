# Release checklist: v0.1.0-rc.11

## Deterministic gates

- [x] Full create, snapshot-read, replace, and delete lifecycle tests pass.
- [x] Stale hashes, symlinks, traversal, ambiguous replacement, binary files,
  bounded reads, and failed nested creation are covered.
- [x] Approval formatting has an exact multiline regression test.
- [x] Full locked test suite, strict Clippy, rustdoc, and Rust 1.88 checks pass.
- [x] Version, lockfile, Debian changelog, manual, installation guide,
  changelog, and release notes agree.

## Publication gates

- [ ] Release archives and packages match their manifest and checksums.
- [ ] Homebrew installs and runs v0.1.0-rc.11 on macOS ARM64.
- [ ] APT preview metadata publishes v0.1.0-rc.11 for amd64 and arm64.
- [ ] Installed binary renders a multiline file diff before approval.
