# Edit transactions

Every modification is prepared against an immutable file snapshot. The request carries the snapshot SHA-256 and an exact UTF-8 byte span. Le Harness generates a candidate and unified diff without touching disk. Applying the preview rechecks the hash immediately before an atomic same-directory rename.

This makes conflicts visible instead of overwriting concurrent human or tool changes. File permissions are preserved. Ordered multi-file change sets stop at the first failure and return both the paths already applied and the failing path; they never imply an all-or-nothing filesystem transaction.

For a new file, `create_file` validates a workspace-relative path, writes the
complete UTF-8 content to a temporary file in the target directory, then
publishes it without replacing an existing file or symlink. Missing parent
directories are created one component at a time; symlinked components are
rejected. Empty directories created by a failed operation are removed.

`read_file` returns the content, size, and SHA-256 needed by the edit tools.
`file_metadata` provides a bounded hash and UTF-8 status without placing binary
content in model context. `replace_text` requires its old text to occur exactly
once. `replace_file` handles intentional complete rewrites. Both recheck the
snapshot before an atomic replacement and preserve existing permissions.

`delete_file` accepts regular text or binary files and requires the expected
SHA-256 from `read_file` or `file_metadata`. It rejects directories and symlinks
and rechecks the hash immediately before removal. The same write permission
policy applies to creation, replacement, FIM editing, and deletion.

In assisted mode, the approval prompt displays a bounded diff for text changes
and a path, size, and hash summary for binary deletion. Approval remains bound
to the exact tool arguments. Runtime events record operation type, path, and
hashes without copying file content into diagnostic metadata.
