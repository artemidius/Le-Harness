# Le Harness v0.1.0-rc.2

This release candidate delivers the complete pre-release runtime:

- native Mistral chat/model discovery and Codestral FIM through Le Rust;
- bounded agent state with explicit planner, FIM, and utility routes;
- repository confinement, hash-bound edit transactions, and Git evidence;
- inspect, assisted, and Docker-backed automated permission modes;
- MCP stdio and loopback OpenAI-compatible local inference adapters;
- deterministic context compaction and edit-strategy evaluation fixtures;
- atomic JSONL session journals, safe continuation, and trace TUI;
- protocol, adversarial, CLI, MSRV, Clippy, rustdoc, and unit coverage.

This is a prerelease. Native execution is not a sandbox, the fixture evaluation
does not establish model quality, and authenticated model checks remain
account-dependent. Read the threat model and limitations before using sensitive
repositories.
