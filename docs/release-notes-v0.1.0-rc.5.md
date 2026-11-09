# Le Harness v0.1.0-rc.5

This release candidate completes interactive first-run cloud setup by adding
account-backed model selection to the existing hidden credential dialog.

## Highlights

- A cloud run with no configured credential first asks for the Mistral API key
  using hidden terminal input. The key remains scoped to the process.
- If chat or FIM model IDs are missing, Le Harness queries the authenticated
  `/v1/models` endpoint and presents numbered terminal choices instead of
  failing on a missing environment variable.
- Chat choices prefer active models that advertise chat and tool-calling
  support. FIM choices prefer active models that advertise FIM support.
- Existing environment variables remain authoritative for repeatable and
  automated runs. JSON, CI, redirected-terminal, and `--no-input` contexts
  remain noninteractive and fail promptly when required configuration is
  absent.
- The pseudo-terminal acceptance fixture verifies hidden input, terminal echo
  restoration, both model dialogs, the selected outbound chat model, and the
  absence of credential persistence.

This is a prerelease. Model availability and capabilities remain
account-dependent, native execution is not a security sandbox, and the
documented external validation gates still apply.
