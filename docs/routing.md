# Model routing

The router has explicit `planner`, `fim`, and `utility` routes. Each route has its own request budget and declared capability. Every chat route emits `model.routed` evidence with the selected backend label and whether a fallback occurred.

Planner-to-utility fallback is opt-in and occurs only when the planner request fails before producing a stream. FIM failures remain visible to the transactional FIM layer, which owns conventional-edit fallback. Model identifiers are supplied by configuration and should be verified with `doctor --live`; Le Harness does not hard-code a claim that an account has a particular alias.
