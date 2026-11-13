# Configuration reference

Le Harness keeps credentials in the process environment and task-specific
settings in CLI flags. It does not load a repository `.env` file.

| Setting | Purpose | Required |
| --- | --- | --- |
| `MISTRAL_API_KEY` | Mistral authentication | Cloud runs and `doctor --live` |
| `LE_HARNESS_CHAT_MODEL` | Planner/chat model ID | Cloud runs |
| `LE_HARNESS_FIM_MODEL` | Codestral-compatible FIM model ID | Cloud runs |
| `LE_HARNESS_UTILITY_MODEL` | Utility model ID | Optional; defaults to chat |
| `LE_HARNESS_LOCAL_API_KEY` | Authentication for a loopback server | Optional |
| `LE_HARNESS_STATE_DIR` | Parent directory for session journals | Optional |

Model IDs are deliberately not guessed. Run `le-harness doctor --live` to query
the authenticated account and compare its returned IDs. Diagnostics report only
whether keys are set.

Permission modes are `inspect`, `assisted`, and `automated-sandbox`. Inspect is
read-only. Assisted requires an exact interactive approval for every mutation,
command, network action, or MCP action. Automated sandbox allows writes and
commands only when `--execution docker` is selected; network and MCP remain
approval-gated.

For local and MCP examples, see [local inference](local-inference.md) and
[MCP](mcp.md).
