# Session journals

Le Harness writes every terminal run as a versioned JSONL journal. The default
directory is `$XDG_STATE_HOME/le-harness/sessions`, or
`$HOME/.local/state/le-harness/sessions` when `XDG_STATE_HOME` is unset.
`LE_HARNESS_STATE_DIR` changes the application state root. The CLI
`--session-dir` option selects the exact journal directory for one command.

Each journal contains exactly one header, an ordered stream of runtime events,
and one footer. Writes use a temporary file, `fsync`, and an atomic rename, so
readers see the old complete state or the new complete state. Invalid schema
versions, discontinuous event sequences, path-like identifiers, and malformed
tool lifecycles fail closed.

## Inspection and continuation

```text
le-harness session list
le-harness session inspect <id>
le-harness session resume <id>
le-harness run "Continue with this follow-up" --resume <id> --root .
le-harness tui <id>
```

`session resume` is a read-only safety check. `run --resume` starts a new child
session containing the prior terminal status and answer plus the new task. It
does not replay the prior transcript or any completed tool operation. A journal
with a requested tool that has no matching completion is reported as requiring
manual reconciliation and cannot be continued automatically. A session also
cannot be resumed against a different repository root.

The TUI reads the same validated journal as the JSON commands. Use `j`/`k` or
the arrow keys to select events, `g`/`G` to jump, `?` for help, and `q` to
restore the terminal and exit. It is intentionally an inspector; approval
prompts remain in the explicit CLI run flow.

Journals are committed only for terminal runs. A process killed before the
atomic commit may leave an ignored temporary file and has no resumable state;
Le Harness does not claim mid-request crash recovery.
