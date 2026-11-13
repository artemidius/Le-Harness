# Limitations and non-goals

Le Harness is a pre-release developer tool. The following boundaries are
intentional and should not be read as missing guarantees:

- Native command execution is not a sandbox. Use Docker isolation for automated
  execution and assess the daemon/image trust boundary separately.
- A worktree is change isolation, not security isolation.
- FIM improves the edit mechanism; it does not prove that one model is more
  capable. The checked-in fixture baseline tests mechanics, not model quality.
- Local OpenAI-compatible endpoints differ. Tool calling and FIM must be
  declared and tested for the selected server/model.
- Session continuation carries forward the terminal answer and status, not a
  lossless hidden model transcript. Completed tools are never replayed.
- MCP servers are local executables with their own trust boundary. Their tools
  must have an explicit effect classification.
- Model availability, identifiers, quotas, and provider behavior are
  account-dependent and require an authenticated live check.
- There is no multi-user daemon, remote control plane, plugin marketplace, or
  claim of production readiness in the preview release.
