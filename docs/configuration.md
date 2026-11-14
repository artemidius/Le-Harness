# Configuration reference

Le Harness accepts Mistral credentials and model IDs from the process
environment or an interactive setup dialog. Prompted values live only for the
current process. It does not save them, mutate the environment, or load a
repository `.env` file.

| Setting | Purpose | Required |
| --- | --- | --- |
| `MISTRAL_API_KEY` | Mistral authentication; otherwise ask in an interactive terminal | Cloud runs and `doctor --live` |
| `LE_HARNESS_CHAT_MODEL` | Planner/chat model ID; otherwise discover and ask interactively | Cloud runs |
| `LE_HARNESS_FIM_MODEL` | Codestral-compatible FIM model ID; otherwise discover and ask interactively | Cloud runs |
| `LE_HARNESS_UTILITY_MODEL` | Utility model ID | Optional; defaults to chat |
| `LE_HARNESS_MISTRAL_BASE_URL` | Alternate Mistral-compatible API root; the Mistral key is sent to this endpoint | Optional; HTTPS, or explicit loopback HTTP |
| `LE_HARNESS_LOCAL_API_KEY` | Authentication for a loopback server | Optional |
| `LE_HARNESS_STATE_DIR` | Parent directory for session journals | Optional |

When a required model ID is absent in an interactive terminal, Le Harness first
resolves the API key, queries the authenticated model list, and presents a
numbered picker. The chat picker prefers active chat models with tool-calling
support; the FIM picker prefers models that advertise FIM. It never substitutes
a hardcoded model name. `LE_HARNESS_UTILITY_MODEL` remains optional and defaults
to the selected chat model. Run `le-harness doctor --live` to print the complete
authenticated model list. Diagnostics report only whether keys are set.

Use the global `--no-input` flag for scripts. It disables credential and model
prompts as well as interactive action approvals; missing input is denied rather
than approved.
`--json`, a truthy `CI` environment variable, or a missing input/diagnostic
terminal also disables credential prompting. Local-only runs never request a
Mistral key.

Permission modes are `inspect`, `assisted`, and `automated-sandbox`. Inspect is
read-only. Assisted requires an exact interactive approval for every mutation,
command, network action, or MCP action. Automated sandbox allows writes and
commands only when `--execution docker` is selected; network and MCP remain
approval-gated.

For local and MCP examples, see [local inference](local-inference.md) and
[MCP](mcp.md).
