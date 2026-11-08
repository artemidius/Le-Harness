# Installation

Le Harness is currently distributed as a release candidate. Package-manager
installs use prebuilt release binaries and do not require a Rust toolchain.

## Homebrew

The project tap supports macOS on Apple Silicon and Linux on x86_64 or ARM64:

```sh
brew install artemidius/tap/le-harness
le-harness --version
le-harness --no-input doctor
```

Upgrade or uninstall it with:

```sh
brew update
brew upgrade artemidius/tap/le-harness
brew uninstall le-harness
```

Intel macOS is not part of the initial binary release matrix.

## APT

The signed APT repository supports Debian 12/13 and Ubuntu 24.04/26.04 on
amd64 and arm64. Release candidates require an explicit opt-in to the
`preview` suite. Follow the [APT bootstrap instructions](install/apt.md) to
install the repository-specific key, deb822 source, and package-scoped
preference. Afterwards normal operations are:

```sh
sudo apt update
sudo apt install le-harness
sudo apt upgrade le-harness
sudo apt remove le-harness
```

Removing the package does not remove the repository configuration. The APT
instructions cover that separate cleanup step.

## Direct release downloads

Every GitHub release publishes SHA-256 checksums, a machine-readable manifest,
and prebuilt archives for:

- macOS ARM64;
- Linux x86_64 and ARM64;
- Windows x86_64.

Linux releases also contain direct `.deb` downloads. A direct package can be
installed without configuring future repository upgrades:

```sh
sudo apt install ./le-harness_0.1.0-rc.4-1_amd64.deb
```

Download `SHA256SUMS` with the artifacts and verify the exact files before
installation:

```sh
grep ' le-harness_0.1.0-rc.4-1_amd64.deb$' SHA256SUMS \
  | sha256sum --check -
```

## First command and credentials

Offline diagnostics require no provider credential:

```sh
le-harness --no-input doctor
```

Configure account-visible model IDs before a cloud run:

```sh
export LE_HARNESS_CHAT_MODEL=...
export LE_HARNESS_FIM_MODEL=...
le-harness run "Explain this repository" --root .
```

If `MISTRAL_API_KEY` is absent, an interactive terminal asks once for hidden
input. The key stays in process memory and is not written to configuration,
session journals, logs, child environments, or shell history. Automation should
provide the variable through its secret manager and disable all prompts:

```sh
export MISTRAL_API_KEY=...
le-harness --no-input run "Inspect the failing test" --mode inspect --root .
```

Do not put the key on a command line. JSON output, recognized CI environments,
`--no-input`, and noninteractive terminals fail promptly when a cloud key is
missing.

Local-only operation needs neither a Mistral key nor cloud model IDs:

```sh
le-harness run "Inspect this repository" --local-only \
  --local-base-url http://127.0.0.1:8000/v1/ \
  --local-model devstral-local
```

Package removal preserves session journals. By default they are under
`$XDG_STATE_HOME/le-harness/sessions`, or
`$HOME/.local/state/le-harness/sessions` when `XDG_STATE_HOME` is unset. Remove
that user-owned data separately only when it is no longer needed.

See the [configuration reference](configuration.md), [validation
matrix](validation.md), and [limitations](limitations.md) for model,
platform, and external validation boundaries.
