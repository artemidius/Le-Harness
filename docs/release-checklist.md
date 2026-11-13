# Release checklist: v0.1.0-rc.2

## Deterministic gates

- [x] All H00-H17 delivery PRs merged independently.
- [x] Rust 1.88 minimum-version check.
- [x] Formatting, strict Clippy, rustdoc warnings, and full locked test suite.
- [x] Protocol fixtures for local inference and MCP stdio.
- [x] Adversarial permission, path, stale-edit, environment, and journal tests.
- [x] Cross-platform tagged-release workflow with SHA-256 checksums.
- [x] MIT license, security policy, contribution guide, changelog, and release notes.

## External gates

- [ ] Authenticated Mistral account model-discovery check.
- [ ] Live Codestral FIM canary against a current account-visible model ID.
- [ ] Representative Ollama or vLLM hardware/model check.
- [ ] Third-party MCP server compatibility check beyond the protocol fixture.

The open gates require credentials, model entitlements, hardware, or third-party
software and are not represented as deterministic CI evidence.
