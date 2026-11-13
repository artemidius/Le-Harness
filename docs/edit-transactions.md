# Edit transactions

Every modification is prepared against an immutable file snapshot. The request carries the snapshot SHA-256 and an exact UTF-8 byte span. Le Harness generates a candidate and unified diff without touching disk. Applying the preview rechecks the hash immediately before an atomic same-directory rename.

This makes conflicts visible instead of overwriting concurrent human or tool changes. File permissions are preserved. Ordered multi-file change sets stop at the first failure and return both the paths already applied and the failing path; they never imply an all-or-nothing filesystem transaction.
