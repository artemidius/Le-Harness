# Release checklist: v0.1.0-rc.10

## Deterministic gates

- [x] New-file path, no-overwrite, and write-policy checks pass.
- [x] Terminal fixture confirms the system instruction and `create_file` tool
  are sent to the provider.
- [x] Full locked test suite, strict Clippy, and Rust 1.88 checks pass.
- [x] Version, lockfile, Debian changelog, manual, installation guide,
  changelog, and release notes agree.

## Publication gates

- [ ] Release archives and packages match their manifest and checksums.
- [ ] Installed macOS binary answers the reported bubble-sort prompt.
- [ ] Homebrew formula and APT preview index publish rc.10.
