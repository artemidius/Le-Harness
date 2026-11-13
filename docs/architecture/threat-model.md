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
- event and diagnostic output excludes content by default;
- stale file snapshots prevent blind overwrites.

A Git worktree separates changes but is not a security sandbox. Container isolation, when available, is a separate execution backend with explicit mounts and network policy.

