# Le Harness v0.1.0-rc.8

Le Harness now starts as a persistent terminal coding-agent chat. Run
`le-harness` in a repository, or provide the first message with
`le-harness run "Explain this repository" --root .`. After each answer, the
same process accepts follow-up messages and retains completed conversation
context, including tool results.

Credentials, model choices, and tool approvals appear in the terminal chat
flow. Enter `/exit` to leave, or press Ctrl-C to cancel the current turn.
Each turn is saved as an inspectable journal linked to the preceding turn.
`--json`, `--no-input`, and noninteractive input retain one-shot behavior for
scripts.

The agent now receives an explicit instruction to inspect relevant repository
files before answering repository questions. The pseudo-terminal acceptance
test covers two turns, context retention, linked journals, hidden credential
input, and clean no-argument startup and exit.

This is a prerelease. Model availability and capabilities depend on the
authenticated account. Native command execution is bounded but is not a
security sandbox.
