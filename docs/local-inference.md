# Local inference

Le Harness can use an OpenAI/Mistral-compatible loopback endpoint. In local-only mode it serves planner, utility, and optionally FIM roles. The adapter deliberately accepts only `localhost` or loopback IP URLs whose base path ends in `/v1/`, so a configuration cannot be mislabeled as local while sending code to a remote host.

If the loopback server requires authentication, set `LE_HARNESS_LOCAL_API_KEY`.
The value is read directly by the client and is not printed by `doctor`.

`--local-only` builds every route from the local adapter and never constructs a cloud client. Without that flag, a configured local endpoint serves the utility role while cloud models retain planner and FIM roles. Capabilities are explicit: tool calling and FIM are never inferred from the model name.

Typical endpoints include a vLLM OpenAI-compatible server or an Ollama-compatible `/v1/` API. Use the exact model identifier returned by `/v1/models`.
