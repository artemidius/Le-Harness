# Le Harness v0.1.0-rc.10

Le Harness now answers code requests directly in chat when no destination file
is specified. A request to write an algorithm no longer needs a repository
file or a special prompt to receive code.

For requests that name a destination, the agent can call `create_file` to save
new UTF-8 files in the workspace. Creation uses the ordinary write approval
policy, rejects paths outside the workspace, and never overwrites an existing
file or symlink.

This is a prerelease. Native command execution is bounded but is not a security
sandbox.
