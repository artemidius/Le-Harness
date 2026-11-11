# Release checklist: v0.1.0-rc.8

## Deterministic gates

- [x] Interactive chat implementation reviewed and merged independently.
- [x] Rust formatting, strict Clippy, rustdoc, locked tests, and Rust 1.88
  compatibility checked by CI.
- [x] PTY fixture verifies two turns in one process, retained tool history,
  linked journals, hidden credential input, and no-argument exit.
- [x] Noninteractive JSON, local inference, MCP, permissions, and session
  regression suites pass.
- [x] Debian package and signed APT repository CI checks pass.
- [x] Version, lockfile, Debian changelog, manual page, installation guide,
  changelog, and release notes agree.

## Live gates

- [ ] Published archives and Debian packages match their release manifest.
- [ ] Homebrew and APT upgrade paths publish the new version.
- [ ] Installed macOS binary opens a chat and completes a follow-up turn.
