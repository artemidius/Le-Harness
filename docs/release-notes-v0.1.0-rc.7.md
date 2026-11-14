# Le Harness v0.1.0-rc.7

This release candidate repairs native Mistral tool-calling loops that failed
after a successful first tool execution.

## Fix

- Replayed assistant tool calls no longer serialize an absent streamed
  `index` as JSON null. Mistral requires an integer whenever that field is
  present and otherwise returns HTTP 422 on the follow-up request.
- Le Harness now uses Le Rust v0.1.3, which omits absent optional tool-call
  fields at the SDK serialization boundary.
- The pseudo-terminal acceptance fixture verifies a complete two-request tool
  loop and rejects `index: null` in the exact outbound replay payload.

Credential persistence introduced in rc.6 is unchanged: interactively entered
keys are stored in the native OS credential store and reused on later runs.

This is a prerelease. Model availability and capabilities remain
account-dependent, and native command execution is bounded but is not a
security sandbox.
