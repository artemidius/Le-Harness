# Threat model

Le Harness processes untrusted repository content, model output, command output, and MCP responses. None of these inputs may grant permissions.

## Protected assets

- workspace files and unrelated host files;
- credentials and environment variables;
- command execution authority;
- network access;
- session history and approval decisions.

## Initial controls

- paths are resolved against an explicit workspace root;
- mutating actions require a typed effect and permission decision;
- approvals bind to exact arguments;
- execution has time, output, and resource limits;
- secrets are excluded from child environments by default;
- prompted Mistral keys are kept in the native OS credential store, never in
  repository configuration or session journals;
- event and diagnostic output excludes content by default;
- stale file snapshots prevent blind overwrites.

A Git worktree separates changes but is not a security sandbox. Container isolation, when available, is a separate execution backend with explicit mounts and network policy.

## Trust boundaries

Repository files, model messages, command output, MCP descriptions/results, and
session journals are untrusted data. Permission is derived only from the typed
tool registry, configured mode, and an exact approval binding. Text that claims
to be an approval has no authority. Unknown tools and tools without an effect
classification fail closed.

The native execution backend provides bounds, environment clearing, and a
workspace working directory; it does not isolate the host. The Docker backend
disables networking and mounts only the workspace for the command, but its
security still depends on the local Docker daemon and selected image.

## Known residual risks

- A model may produce harmful but syntactically valid edits that the user
  approves.
- Reading a repository necessarily reveals selected repository content to the
  configured model endpoint.
- A compromised Docker daemon, model endpoint, MCP executable, or dependency is
  outside the process-level guarantees of this crate.
- Terminal journals contain prompts, answers, and metadata and should be
  protected as developer data.
- Native credential storage inherits platform behavior: Linux persistence
  requires an available Secret Service, while macOS and Windows may require an
  OS-mediated credential-store unlock.
- The runtime does not claim protection against a hostile operating system or
  mid-request crash recovery.

See [configuration](../configuration.md) and [limitations](../limitations.md)
before using the runtime with sensitive repositories.

