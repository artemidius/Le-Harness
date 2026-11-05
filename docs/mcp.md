# MCP client

Le Harness uses the official Rust MCP SDK for local standard-I/O servers. Server commands are explicit argument vectors, their environments are cleared, initialization/list/call operations are timed out, and returned payloads are size bounded.

Every discovered tool must have an operator-configured effect. Server annotations are retained as protocol metadata but never grant permissions because the MCP specification treats them as hints. Tools are exposed to the model under a collision-checked `mcp__<server>__<tool>` name and still pass through the ordinary argument-bound approval policy.

Example configuration:

```json
{
  "servers": {
    "git": {
      "program": "uvx",
      "args": ["mcp-server-git", "--repository", "."],
      "tool_effects": { "git_status": "read" }
    }
  }
}
```
