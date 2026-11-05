# Context management

Le Harness labels context size as an estimate unless the provider returns usage. The local estimator is deterministic (`ceil(serialized UTF-8 bytes / 4)`) and exists to trigger consistent compaction, not to impersonate Mistral tokenization.

Compaction preserves the original task, keeps recent messages in semantic units, and never separates an assistant tool-call message from its following tool results. Older units become bounded metadata (roles, sizes, tool names); raw tool output is never elevated into a system message. If the original task alone exceeds the configured estimate, the runtime reports that the compacted context remains over budget instead of truncating the task silently.

Repository excerpts are path-labeled and clipped on UTF-8 boundaries before they enter a prompt.
