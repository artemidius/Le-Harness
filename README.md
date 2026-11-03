# Le Harness

[![CI](https://github.com/artemidius/Le-Harness/actions/workflows/ci.yml/badge.svg)](https://github.com/artemidius/Le-Harness/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/artemidius/Le-Harness/blob/main/LICENSE)

Le Harness is a Rust coding-agent runtime for the Mistral ecosystem. It is designed around transactional edits, native Codestral fill-in-the-middle requests, explicit permissions, and inspectable execution traces.

The project is under active development. Its current version is not a production release.

## Provider configuration

Le Harness uses the tagged Le Rust SDK for native Mistral model discovery, streamed chat, and FIM completions. Configure the provider without placing credentials in repository files:

```text
export MISTRAL_API_KEY=...
export LE_HARNESS_CHAT_MODEL=...
export LE_HARNESS_FIM_MODEL=...
le-harness doctor --live
```

`doctor` reports whether credentials are present but never prints their value. Model identifiers stay explicit because availability varies by account.

## Design boundaries

- [Le Rust](https://github.com/artemidius/Le-Rust) owns Mistral API transport and native wire types.
- Le Harness owns agent state, context, model routing, permissions, tools, filesystem changes, MCP, and evaluation.
- Model output never grants authority to modify a workspace or execute a command.
- Runtime events use a versioned JSON envelope so sessions and machine-readable output remain inspectable.

See the [architecture](docs/architecture/overview.md), [threat model](docs/architecture/threat-model.md), and [delivery plan](docs/implementation-plan.md).

## Development

The minimum supported Rust version is 1.88.

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

## License

Licensed under the [MIT License](https://github.com/artemidius/Le-Harness/blob/main/LICENSE).

