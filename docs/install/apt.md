# Install with APT

The project repository is at
`https://artemidius.github.io/Le-Harness/apt/`. Release candidates are in the
opt-in `preview` suite; stable releases will use the separate `stable` suite.

## Add the preview repository

Install the archive key into a repository-specific keyring:

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://artemidius.github.io/Le-Harness/apt/le-harness-archive-keyring.gpg \
  | sudo tee /etc/apt/keyrings/le-harness-archive-keyring.gpg >/dev/null
sudo chmod 0644 /etc/apt/keyrings/le-harness-archive-keyring.gpg
gpg --show-keys --with-fingerprint /etc/apt/keyrings/le-harness-archive-keyring.gpg
```

Confirm the full fingerprint before continuing:

```text
520A 6749 CEFE 62FC EEA3  0447 2160 9ACA AA5F 447E
```

Add the deb822 source and a package-specific preference:

```sh
sudo tee /etc/apt/sources.list.d/le-harness.sources >/dev/null <<'EOF'
Types: deb
URIs: https://artemidius.github.io/Le-Harness/apt/
Suites: preview
Components: main
Architectures: amd64 arm64
Signed-By: /etc/apt/keyrings/le-harness-archive-keyring.gpg
EOF

sudo tee /etc/apt/preferences.d/le-harness >/dev/null <<'EOF'
Package: *
Pin: release o=Le Harness
Pin-Priority: 1

Package: le-harness
Pin: release o=Le Harness
Pin-Priority: 500
EOF

sudo apt update
sudo apt install le-harness
```

The source uses `Signed-By`; do not add the repository with `apt-key`,
`trusted=yes`, or disabled signature checks. The low wildcard priority keeps
this repository from changing candidates for unrelated packages, while the
package-specific rule enables `le-harness`.

Use `sudo apt update && sudo apt upgrade le-harness` for later updates. Stable
users should replace `Suites: preview` with `Suites: stable` once a stable
release is published.

## Remove the package or repository

Removing the package does not remove its source configuration:

```sh
sudo apt remove le-harness
```

To stop using the repository, remove its source, preference, and dedicated key,
then refresh APT:

```sh
sudo rm /etc/apt/sources.list.d/le-harness.sources
sudo rm /etc/apt/preferences.d/le-harness
sudo rm /etc/apt/keyrings/le-harness-archive-keyring.gpg
sudo apt update
```

Repository metadata is valid for 30 days and is refreshed weekly from the same
published release artifacts. Maintainers can run the **Publish APT repository**
workflow manually with an explicit published tag if a refresh or recovery is
needed.
