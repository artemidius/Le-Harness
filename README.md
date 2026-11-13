# Le Harness

[![CI](https://github.com/artemidius/Le-Harness/actions/workflows/ci.yml/badge.svg)](https://github.com/artemidius/Le-Harness/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/artemidius/Le-Harness/blob/main/LICENSE)

Le Harness is a Rust coding-agent runtime for the Mistral ecosystem. It is designed around transactional edits, native Codestral fill-in-the-middle requests, explicit permissions, and inspectable execution traces.

The current version is a release candidate, not a production release.

## Install

Install the prebuilt release from the project-owned Homebrew tap:

```text
brew tap artemidius/tap
brew trust --formula artemidius/tap/le-harness
brew install artemidius/tap/le-harness
```

The formula-scoped trust applies only to Le Harness, not to other current or
future formulae in the tap.

Debian and Ubuntu users can opt into the signed preview APT repository, then
install and upgrade with ordinary `apt` commands. Direct archives and `.deb`
packages remain available on GitHub Releases. See the [installation
guide](docs/installation.md) for supported platforms, signature setup,
upgrades, removal, and first-run credential behavior.

## Provider configuration

Le Harness uses the tagged Le Rust SDK for native Mistral model discovery,
streamed chat, and FIM completions. Model IDs stay explicit because availability
varies by account:

```text
export LE_HARNESS_CHAT_MODEL=...
export LE_HARNESS_FIM_MODEL=...
# Optional; defaults to LE_HARNESS_CHAT_MODEL
export LE_HARNESS_UTILITY_MODEL=...
le-harness doctor --live
```

When `MISTRAL_API_KEY` is not set, an interactive cloud command asks once for
hidden input and keeps the key only for that process. Automation should export
the variable and pass `--no-input`; JSON and recognized CI contexts never wait
for a prompt. `doctor` reports whether an environment credential is present but
never prints its value.

## Run a session

The default assisted mode allows repository reads and asks before each exact write or command:

```text
le-harness run "Explain the failing test and fix it" --root .
```

Use `--mode inspect` for a read-only session. Automated execution is accepted only with the Docker backend:

```text
le-harness run "Run the test suite and repair the failure" \
  --mode automated-sandbox --execution docker
```

`--json` emits the terminal state, answer, versioned runtime events, counters, and Git before/after evidence. It is deliberately non-interactive, so approval-requiring operations are denied.

To keep every model request on a loopback OpenAI-compatible server:

```text
le-harness run "Inspect this repository" --local-only \
  --local-base-url http://127.0.0.1:8000/v1/ \
  --local-model devstral-local
```

Add `--local-fim-model <id>` only when the server implements the compatible FIM endpoint. Without `--local-only`, the configured local model is used for the utility route and cloud planner/FIM configuration remains required.

Every run is persisted as a versioned JSONL journal. List and inspect sessions,
or safely continue a terminal session, without replaying its completed tools:

```text
le-harness session list
le-harness session inspect <session-id>
le-harness run "Now add the regression test" --resume <session-id> --root .
le-harness tui <session-id>
```

Use `--session-dir` to select an exact journal directory or
`LE_HARNESS_STATE_DIR` to relocate all state. See the [session journal
contract](docs/sessions.md) for resume and crash-safety boundaries.

The TUI is a keyboard-driven trace inspector: it shows the ordered runtime
events, the selected event's structured payload, terminal status, and answer.

## Design boundaries

- [Le Rust](https://github.com/artemidius/Le-Rust) owns Mistral API transport and native wire types.
- Le Harness owns agent state, context, model routing, permissions, tools, filesystem changes, MCP, and evaluation.
- Model output never grants authority to modify a workspace or execute a command.
- Runtime events use a versioned JSON envelope so sessions and machine-readable output remain inspectable.

See the [architecture](docs/architecture/overview.md), [threat model](docs/architecture/threat-model.md), [configuration reference](docs/configuration.md), [installation guide](docs/installation.md), [limitations](docs/limitations.md), [validation matrix](docs/validation.md), [release process](docs/release-process.md), and [delivery plan](docs/implementation-plan.md).

Preview binaries and checksums are published on the [GitHub Releases](https://github.com/artemidius/Le-Harness/releases) page.

## Development

The minimum supported Rust version is 1.88.

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
```

## License

Licensed under the [MIT License](https://github.com/artemidius/Le-Harness/blob/main/LICENSE).

