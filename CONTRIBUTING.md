# Contributing

Use one focused branch and pull request per delivery-plan item. New behavior includes relevant tests, documentation, and an explanation of failure behavior.

Before opening a pull request, run the commands in `README.md`. Never commit API keys, repository content captured from private workspaces, or raw model transcripts containing secrets.

Security-sensitive changes should include an adversarial test. Keep provider
transport changes in Le Rust; Le Harness changes should operate through its
public client boundary. Document any validation that requires credentials,
specific hardware, or third-party services as an open external gate.

