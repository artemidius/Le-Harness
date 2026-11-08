# Release checklist: v0.1.0-rc.4

## Deterministic gates

- [x] Interactive credential and packaging changes merged independently.
- [x] Rust 1.88 minimum-version check.
- [x] Formatting, strict Clippy, rustdoc warnings, and full locked test suite.
- [x] Protocol fixtures for local inference and MCP stdio.
- [x] Adversarial permission, path, stale-edit, environment, and journal tests.
- [x] Cross-platform tagged-release workflow with manifest and SHA-256 checksums.
- [x] Debian package lifecycle tests on Debian 12/13 and Ubuntu 24.04/26.04.
- [x] Signed APT metadata, package hash, and tamper-rejection tests.
- [x] Release asset names round-trip through GitHub without normalization.
- [x] Hidden credential PTY and noninteractive fail-fast tests.
- [x] Public APT installs on Debian 12 ARM64 and Ubuntu 24.04 amd64.
- [x] Public Homebrew formula matrix on macOS ARM64 and Linux amd64/arm64.
- [x] Idempotent, manifest-driven Homebrew update workflow with scoped tap access.
- [x] MIT license, security policy, contribution guide, changelog, and release notes.

## External gates

- [ ] Authenticated Mistral account model-discovery check.
- [ ] Live Codestral FIM canary against a current account-visible model ID.
- [ ] Representative Ollama or vLLM hardware/model check.
- [ ] Third-party MCP server compatibility check beyond the protocol fixture.
- [ ] Windows console acceptance check for hidden credential input.

The open gates require credentials, model entitlements, hardware, or third-party
software and are not represented as deterministic CI evidence.
