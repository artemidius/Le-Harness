# Release checklist: v0.1.0-rc.9

## Deterministic gates

- [x] Model persistence and in-chat switching reviewed independently.
- [x] Full locked test suite and strict Clippy pass.
- [x] PTY fixture verifies model selection, immediate switching, persisted
  choices, restart without a picker, and hidden credential input.
- [x] Version, lockfile, Debian changelog, manual, installation guide,
  changelog, and release notes agree.
- [x] Rust 1.88, Debian package, and signed APT repository CI checks pass.

## Publication gates

- [ ] Release archives and packages match their manifest and checksums.
- [ ] Homebrew formula and APT preview index publish rc.9.
- [ ] Installed macOS binary reuses a saved model choice.
