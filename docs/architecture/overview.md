# Architecture

Le Harness uses one session controller as the owner of mutable runtime state. Model adapters, tools, execution backends, persistence, and user interfaces exchange typed events with that controller. They do not independently mutate the conversation or workspace.

```text
CLI / TUI
    |
Session controller
    |-- context builder
    |-- model router -> Le Rust -> Mistral
    |-- tool dispatcher -> permission policy -> execution backend
    |-- edit transaction manager
    `-- event journal and usage accounting
```

The first release remains one Cargo package with a library and binary. A crate split requires a concrete downstream or compilation boundary.

## Ownership

Le Rust owns authentication, Mistral HTTP transport, native chat/FIM types, SSE decoding, provider metadata, and transport errors. Le Harness owns configuration UX, state transitions, budgets, permissions, tool execution, edits, routing, persistence, and evaluation.

