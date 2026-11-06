# Validation matrix

| Surface | Deterministic evidence | External gate |
| --- | --- | --- |
| Agent loop | scripted streaming, tool-loop, budget, cancellation tests | authenticated model response |
| Repository tools | traversal, symlink escape, text/size limit tests | representative large repository |
| Edit engine | stale hash, exact span, CRLF, mode, partial change-set tests | model-generated patches |
| Execution | environment, timeout, output, network-policy tests | Docker daemon/image posture |
| FIM | fixture completion through the transaction engine | current Codestral FIM model |
| Routing | capability, budget, and fallback event tests | configured cloud/local models |
| MCP | stdio discovery/call fixture and effect rejection | selected third-party server |
| Local inference | loopback discovery, stream, and FIM protocol fixture | Ollama/vLLM model behavior |
| Sessions/TUI | journal tamper/resume and test-backend render tests | interactive terminal check |

The CI quality job runs formatting, strict Clippy, documentation, and all tests
with the lockfile. A separate job checks every target and feature on Rust 1.88,
the declared minimum supported version.
