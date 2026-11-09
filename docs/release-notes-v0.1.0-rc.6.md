# Le Harness v0.1.0-rc.6

This release candidate makes interactive cloud setup reusable and repairs
native Mistral tool-calling conversations after the first tool result.

## Highlights

- A hidden, interactively entered Mistral API key is saved in the native OS
  credential store and reused on later runs. Environment keys remain
  authoritative and are never copied into the store.
- `le-harness auth login`, `auth status`, and `auth logout` explicitly manage
  the saved credential without putting it on a command line. The global
  `--no-credential-store` flag keeps an invocation ephemeral.
- macOS uses Keychain, Windows uses Credential Manager, and Linux uses a
  Secret Service-backed user keyring.
- Tool-result messages now retain both the original function name and tool-call
  ID, matching Mistral's documented function-calling loop. This fixes the HTTP
  422 seen after a successful first tool such as `list_files`.
- Failed agent runs now return a non-zero process status and print the bounded,
  redacted provider error instead of showing only `status: failed`.
- The pseudo-terminal acceptance fixture now completes a two-request tool loop
  and asserts the exact outbound tool-result message without using a real
  credential store.

This is a prerelease. Model availability and capabilities remain
account-dependent, native execution is not a security sandbox, and headless
Linux systems require an available Secret Service for persistent key storage.
