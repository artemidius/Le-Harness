# Le Harness v0.1.0-rc.11

Le Harness can now complete the full repository file lifecycle. Agent turns can
read hash-bearing snapshots, create files and missing parent directories,
perform localized or complete UTF-8 replacements, inspect binary metadata, and
delete files. Changes and deletions require the current SHA-256, so stale model
actions fail instead of overwriting newer work.

Interactive write approval now shows a real multiline diff and a separate
confirmation prompt. It no longer prints file contents as an escaped JSON
string. Approval remains bound to the exact tool name and arguments.

File operations stay inside the selected workspace, reject symlink traversal,
bound content and preview sizes, and emit structured operation events without
copying file contents into diagnostics.

This is a prerelease. Native command execution is bounded but is not a security
sandbox.
