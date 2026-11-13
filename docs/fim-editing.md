# Codestral FIM editing

FIM is used only for a planner-selected span in a verified snapshot. Le Harness sends the exact bytes before the span as the prefix and the bytes after it as the suffix. The returned middle is bounded, checked as text, previewed through the ordinary transaction engine, and applied only after its snapshot hash is revalidated.

If the FIM request or generated middle fails validation, Le Harness can use a caller-supplied conventional replacement. The result records `fim` or `conventional_fallback` and, for fallback, the triggering reason. It never silently changes edit strategy.
