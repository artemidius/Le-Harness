# Edit transactions

Every modification is prepared against an immutable file snapshot. The request carries the snapshot SHA-256 and an exact UTF-8 byte span. Le Harness generates a candidate and unified diff without touching disk. Applying the preview rechecks the hash immediately before an atomic same-directory rename.

This makes conflicts visible instead of overwriting concurrent human or tool changes. File permissions are preserved. Ordered multi-file change sets stop at the first failure and return both the paths already applied and the failing path; they never imply an all-or-nothing filesystem transaction.

For a new file, `create_file` validates a workspace-relative path, writes the
complete UTF-8 content to a temporary file in the target directory, then
publishes it without replacing an existing file or symlink. The parent directory
must already exist. The same write permission policy applies to creation and
edits.
