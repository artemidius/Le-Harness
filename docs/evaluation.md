# Evaluation protocol

`le-harness eval` reads a versioned JSON manifest and runs each case the same number of times for one selected strategy. JSON output is suitable for aggregation; Markdown is intended for checked-in reports.

The bundled smoke suite is deterministic and tests the measurement pipeline. A model benchmark must additionally record exact model identifiers, endpoint/backend, prompt version, tool budget, context budget, repository revision, repetitions, failures, and raw run traces. Comparisons are valid only when all variables other than edit strategy are held constant.
