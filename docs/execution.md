# Command execution

Le Harness exposes command execution through bounded backends, never through an implicit shell. A request contains an executable and an argument vector. The caller must separately authorize the `execute` effect and, when requested, the `network` effect.

## Native backend

The native backend clears the child environment, restores only configured variables, confines the working directory to the repository, applies a wall-clock timeout, and captures bounded output. It is **not a security sandbox**: the child has the operating-system privileges of Le Harness.

## Docker backend

The Docker backend provides the isolation option. It drops Linux capabilities, enables `no-new-privileges`, limits processes, memory, and CPUs, and disables networking by default. The repository is the only mounted workspace. Enabling network access requires both backend configuration and a separate policy approval.

Docker availability is a runtime capability. If the daemon or configured image is unavailable, Le Harness reports that limitation and does not silently fall back to native execution.
