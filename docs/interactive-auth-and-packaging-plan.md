# Interactive credentials and package-manager distribution

Status: accepted implementation plan. Delivery is tracked through the focused
pull requests below.

## Objectives

1. Prompt for a Mistral API key when a cloud command needs one and
   `MISTRAL_API_KEY` is missing or blank.
2. Install and upgrade Le Harness through a project-owned Homebrew tap.
3. Install and upgrade Le Harness through a signed APT repository, with direct
   `.deb` downloads available as well.
4. Deliver the work as nine focused pull requests, each with acceptance criteria.

## Verified starting point

- The application is Rust 2024 with MSRV 1.88 and consumes Le Rust v0.1.1.
- `src/settings.rs` requires the API key and model identifiers from environment
  variables. `src/main.rs` calls this loader for cloud runs and `doctor --live`.
- Local-only runs construct no cloud client. Plain `doctor`, evaluation,
  session inspection, and the trace TUI do not require cloud authentication.
- `run --json` currently disables interactive approvals.
- Release v0.1.0-rc.2 contains macOS ARM64, Linux x86_64, and Windows x86_64
  archives plus checksums. No `.deb` artifacts exist.
- The release workflow builds Linux on Ubuntu 24.04, always publishes a
  prerelease, and hardcodes the rc.2 release-notes path.
- No matching Homebrew tap was found in the account's repository listing.
  The Le-Harness Pages API returned 404; Pages setup must be checked during
  implementation.

## Product contract

### Credential behavior

| Invocation/context | Behavior when `MISTRAL_API_KEY` is missing or blank |
| --- | --- |
| Interactive cloud `run` | Ask once for hidden input; continue with that key |
| Interactive hybrid `run` | Same behavior, since the planner/FIM use cloud APIs |
| Interactive `doctor --live` | Ask for hidden input before model discovery |
| `run --json` | Exit with an actionable error; never wait for input |
| `--no-input`, recognized CI mode, or unavailable interactive terminal | Exit with an actionable error |
| `run --local-only` | No Mistral prompt or Mistral discovery request |
| Plain `doctor`, local-only diagnostic probe, `eval`, `session`, `tui`, help/version | No Mistral prompt |
| Any cloud command with a nonblank environment key | Use that key without prompting |

Example, assuming model configuration is already valid:

```text
$ le-harness run "Explain this repository"
Mistral API key (hidden; used for this process only):
```

Use the terminal/console for hidden input and the diagnostic stream for the
prompt. Do not read a token from an arbitrary stdin pipe. Require interactive
input and diagnostic output; redirecting stdout alone need not disable the
human flow. JSON mode and `--no-input` always override terminal availability.

The key remains in process memory for the command. Do not save it in a session,
configuration file, `.env`, history, or global environment. Keep it out of
Debug output, logs, child-command environments, and serialized events. Avoid
introducing a command-line key value that would appear in process arguments.

Reject blank input and embedded control characters with a concise error. EOF
and cancellation abort cleanly and restore terminal state. No unbounded prompt
or authentication retry loop. Provider errors must distinguish authentication
failure, permission/model access failure, rate limits, and transport failure.

Model IDs remain explicit configuration. This task does not introduce guessed
defaults or a model-selection wizard. Improve `doctor --live` so it can list
available IDs with just a key and compare whichever role IDs are configured.
Validate required run configuration before collecting a secret unnecessarily.

### Architecture boundary

Keep terminal interaction in the Le Harness CLI. Le Rust continues to accept
an explicit key through its client builder and stays usable in non-interactive
applications.

Split configuration into a pure model/environment parser and an explicit
credential resolver. Preserve an environment-only library constructor for
callers that want current behavior. Inject the resolved key into settings and
clients directly; do not mutate process environment variables.

Use a narrow hidden-input adapter, backed by a maintained cross-platform crate
such as [rpassword](https://docs.rs/rpassword/latest/rpassword/). Select and pin
a version that builds on MSRV 1.88. Keep the resolution policy independently
testable using injected environment values and a fake input adapter.

`--no-input` should suppress all user prompts, including action approvals.
Suppressing an approval means denying the action, never auto-approving it.
Credential collection happens during startup before MCP processes, the agent
loop, or workspace mutations begin.

### Distribution ownership

| Repository | Responsibility |
| --- | --- |
| `artemidius/Le-Harness` | Rust code, release artifacts, Debian packaging, APT repository generation/deployment, installation docs |
| Proposed `artemidius/homebrew-tap` | Formula, installation tests, reviewed version/checksum updates |

Homebrew permits project-owned taps; installation would be:

```sh
brew install artemidius/tap/le-harness
brew upgrade artemidius/tap/le-harness
```

This does not require acceptance into `homebrew/core`. Core requires a stable
upstream release, so core submission is outside this task. A release candidate
can be distributed through the custom tap with its actual version and clear
release status. Sources: [tap documentation](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)
and [core requirements](https://docs.brew.sh/Acceptable-Formulae).

APT requires one-time repository setup; afterwards users can run:

```sh
sudo apt update
sudo apt install le-harness
sudo apt upgrade le-harness
```

A downloadable `.deb` also supports `sudo apt install ./le-harness_<version>_<arch>.deb`,
but that alone does not configure future upgrades.

Proposed APT base URL: `https://artemidius.github.io/Le-Harness/apt/`.
This is a planned URL and is not available yet. Publish static APT content
through GitHub Pages from the application repository. Do not commit package
binaries into source control. Measure generated site size and retained package
history against [Pages limits](https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits).

### Initial platform and channel policy

| Installer | Initial tested targets |
| --- | --- |
| Homebrew | macOS ARM64; Linux x86_64 and ARM64 |
| APT | Debian 12/13 and Ubuntu 24.04/26.04, each on amd64 and arm64 |
| Existing archive downloads | Preserve Windows x86_64 and current archive support |

Build GNU/Linux artifacts in a pinned Debian 12 baseline for each architecture,
then inspect their actual ELF/runtime dependencies and test the built packages
on every advertised distribution. Do not infer Debian compatibility from an
Ubuntu build or use CPU-specific compiler tuning. Older Ubuntu versions and
Intel macOS are not part of the initial package support claim.

Use a `preview` APT suite for release candidates and a separate `stable` suite
for stable releases. The tap initially ships the declared release candidate;
after its first stable release, automatic formula updates must not replace a
stable version with a prerelease. Future preview-formula support can be added
separately if needed.

Preserve Debian version ordering. For example:

```text
Cargo/tag: 0.1.0-rc.3 / v0.1.0-rc.3
Debian:   0.1.0~rc.3-1
Stable:   0.1.0-1
```

Test ordering with `dpkg --compare-versions`, including rc.2, rc.3, rc.10,
stable, and packaging-only revisions. See [Debian version rules](https://www.debian.org/doc/debian-policy/ch-controlfields.html#version).

## Pull-request implementation sequence

### P01 — Explicit credential resolution

Repository: Le-Harness. Dependencies: none.

Deliverables:

- Refactor `src/settings.rs` to separate optional environment credentials from
  model configuration and accept an explicitly supplied key.
- Introduce a small redacted credential type and a resolver taking environment
  input, interaction policy, and a prompt interface.
- Preserve the environment-only constructor; add typed missing-key,
  input-cancelled, and invalid-input errors.
- Unit-test environment precedence, missing/empty/whitespace values,
  malformed input, redacted Debug formatting, and disabled prompting.

Acceptance: callers can supply a key without writing environment variables;
existing environment-based calls continue to work; the library never opens a
terminal or waits for input itself.

### P02 — Hidden CLI prompt and terminal acceptance tests

Repository: Le-Harness. Dependencies: P01.

Deliverables:

- Integrate hidden input into cloud/hybrid runs and `doctor --live`.
- Add global `--no-input`; unify its behavior with JSON/CI and approval handling.
- Keep local and offline workflows credential-free.
- Allow live model discovery before role IDs have been configured.
- Add process tests with a fake provider and pseudo-terminal tests verifying
  hidden typing, cancellation/EOF, terminal restoration, and one prompt per
  command. Exercise the supported Windows console path on an available
  console runner or record that check explicitly until it is exercised.
- Verify the entered sentinel key reaches only the intended Authorization
  header and is absent from output, session journals, logs, and child envs.
- Update authentication examples and errors.

Acceptance: a developer with configured model IDs can run without exporting a
key, paste it invisibly, and complete a fixture-backed run. JSON/noninteractive
cases terminate promptly rather than hang, and local-only cases never prompt.
No paid live request is necessary for deterministic acceptance.

### P03 — Versioned release artifacts and compatible Linux builds

Repository: Le-Harness. Dependencies: none; package publication waits for P02.

Deliverables:

- Validate tag, Cargo version, lockfile version, and selected release notes.
- Remove the hardcoded notes filename and unconditional prerelease flag.
- Add Linux ARM64 builds and the pinned baseline for both Linux architectures.
- Produce versioned archives and a machine-readable release manifest listing
  source commit, version/channel, target, artifact name, size, and SHA-256.
- Run packaged-binary smoke tests without Mistral credentials.
- Stage publication as a draft until required artifacts and checks complete;
  handle reruns by verifying existing assets instead of replacing them silently.
- Restrict write permissions to publication jobs and prevent concurrent
  publishing from racing.

Acceptance: one tagged commit yields verified assets for each declared target;
an invalid tag/version pair or missing target prevents package promotion.

### P04 — Debian packages

Repository: Le-Harness. Dependencies: P03.

Deliverables:

- Configure a pinned [cargo-deb](https://github.com/kornelski/cargo-deb) tool
  and package metadata using the release binaries already built for each target.
- Include `/usr/bin/le-harness`, MIT copyright/license material, and packaged
  release documentation under `/usr/share/doc/le-harness/`.
- Derive shared-library dependencies from the actual binary. Include CA
  certificates for HTTPS and Git for Git integration; keep Docker an optional
  backend dependency. Installation itself requires no Rust toolchain.
- Use verified maintainer identity and an approved public contact already
  associated with the project; do not infer private contact details.
- Add version conversion and inspect package contents, architecture, modes,
  dependencies, and metadata with dpkg/Lintian.
- Test fresh install, upgrade, removal, and purge in clean supported systems,
  preserving user-owned session data.

Acceptance: `apt install ./<artifact>.deb` installs the correct binary and
dependencies. Package scripts neither request an API key nor launch the agent.

### P05 — Homebrew tap

Repository: new homebrew-tap. Dependencies: P03.

Deliverables:

- Add `Formula/le-harness.rb` with explicit version, homepage, MIT license,
  supported platform selection, immutable release URLs, and per-target hashes.
- Use the verified upstream binaries in this custom tap; do not describe a
  generic archive as a Homebrew bottle. Require no Rust compilation at install.
- Declare necessary runtime dependencies and unsupported-target behavior.
- Add `brew style`, applicable tap audits, real install/uninstall checks, and
  `brew test` covering version and a useful offline command.
- Add concise installation/upgrade instructions and release-status caveats.

Acceptance: the fully qualified install command works on each advertised
Homebrew target, package hashes match the release manifest, and installation
never asks for provider credentials.

### P06 — Signed APT repository builder

Repository: Le-Harness. Dependencies: P04.

Deliverables:

- Generate the conventional `pool/` and `dists/<suite>/main/binary-<arch>/`
  layout with Packages indexes, compressed indexes, Release, InRelease, and
  Release.gpg.
- Include immutable package paths, checksums, suite/component/architecture
  metadata, and by-hash indexes; retain previous referenced objects through
  the defined metadata/cache validity window.
- Implement separate preview/stable promotion and retention rules with an
  explicit generated-site size bound.
- Use a dedicated repository signing identity. Keep its primary private key
  outside CI; store only the required signing subkey in protected release
  secrets. Document fingerprint, backup, rotation, revocation, and expiry.
- Make fork/PR validation use disposable test keys and exclude production
  signing secrets from untrusted workflows.
- Test the signed repository locally: normal install succeeds; altered or
  unsigned metadata and packages fail verification.

Acceptance: a clean APT client verifies repository metadata and package hashes,
selects the correct architecture/version, and never needs trust checks disabled.
APT authenticates the signed repository metadata and its checksum chain; a
detached signature on a `.deb` alone does not provide this installation flow.
See [apt-secure](https://manpages.debian.org/trixie/apt/apt-secure.8.en.html).

### P07 — Public APT hosting and bootstrap instructions

Repository: Le-Harness. Dependencies: P06.

Deliverables:

- Enable/reuse Pages after checking its current configuration and publish a
  complete staged repository snapshot through a serialized deployment job.
- Publish the public signing key, fingerprint, and install instructions.
- Provide a deb822 `.sources` configuration using a repository-specific key in
  `/etc/apt/keyrings/` with `Signed-By`; never use `apt-key` or `trusted=yes`.
- Scope package preferences so the new repository is selected for Le Harness,
  not unrelated packages. Explicitly document opting into the preview suite.
- Document removal of the repository configuration separately from package
  removal.
- Define metadata freshness, for example 30-day validity refreshed weekly
  using the same verified package inventory; alert on failed refreshes before
  expiry. Include a manual refresh path.
- Run real clean-system install and upgrade checks against the public HTTPS
  endpoint after deployment and handle deployment/CDN propagation explicitly.

Acceptance: after documented one-time setup, `apt update` and
`apt install le-harness` succeed with signature verification enabled and no
unrelated package candidates changed. Use the current
[APT source format](https://manpages.debian.org/trixie/apt/sources.list.5.en.html).

### P08 — Automated package updates

Repositories: Le-Harness and homebrew-tap. Dependencies: P05 and P07.

Deliverables:

- Make the release manifest the single version/hash input for both installers.
- Promote only completed releases with all required validation evidence.
- Generate a focused tap update PR and run tap tests before merge. Provision a
  credential scoped to that repository for automation; the application's
  default workflow token is not assumed to have cross-repository permissions.
- Update APT suites from the same validated artifacts, preserving prior
  installable versions according to retention policy.
- Test missing assets, checksum mismatch, signing failure, interrupted deploy,
  duplicate invocation, and preview-to-stable ordering.
- On failure, leave the last successful package distribution available and
  emit an actionable failed job. Do not rebuild different bytes under an
  existing release version.

Acceptance: the next release updates both package routes through repeatable
jobs, and reruns are safe. A failed candidate cannot overwrite a working
distribution or silently change its channel.

### P09 — Installation documentation and release verification

Repositories: Le-Harness; tap README if necessary. Dependencies: P02 and P08.

Deliverables:

- Publish `docs/installation.md` and update README, configuration, changelog,
  and release checklist with verified commands and supported-platform bounds.
- Cover Brew, APT, direct `.deb`, upgrades/removal, preview status, hidden key
  entry, environment usage for automation, and local-only usage.
- Run fresh-install-to-first-command tests through both public installers;
  verify version, offline doctor, prompt behavior, and fixture-backed requests.
- Record actual tested versions/architectures and distinguish any outstanding
  console or live-provider checks from completed checks.
- Publish the next immutable release candidate, suggested v0.1.0-rc.3 if still
  available, with archives, `.deb` assets, hashes, and package instructions.
- Review each implementation PR and merge it only after its relevant checks
  pass; record installer publication evidence after the final release.

Acceptance: a new user can follow the published instructions, install, enter a
key without exporting it, and use the tool. Both installers can upgrade to the
next version; cleanup retains user data.

## Dependency map and completion gates

P01 -> P02; P03 -> P04 -> P06 -> P07; P03 -> P05;
P05 + P07 -> P08; P02 + P08 -> P09.

All application changes retain MSRV 1.88 and pass formatting, locked tests,
Clippy, and documentation checks. Package tests execute the artifacts users
will install. Adding files or producing a `.deb` without testing installation
does not satisfy the distribution acceptance criteria.

Before production publication, verify Pages availability, repository write
access for the tap, signing-key custody and protected secrets, a public package
maintainer contact, and supported build runners. These are concrete setup
requirements for implementation; this plan does not create them.

Persistent credential storage, OS keychains, Homebrew core submission, official
Debian/Ubuntu archive inclusion, Snap/Flatpak, and automatic self-updating are
outside this delivery. Account-backed interactive model selection was added in
a subsequent release.
