# Le Harness v0.1.0-rc.3

This release candidate adds secure interactive credentials and native package
distribution to the existing Mistral-oriented agent runtime.

## Highlights

- Interactive cloud commands ask once for a hidden Mistral API key when the
  environment variable is absent. The key stays in process memory and is not
  written to session journals, configuration, child environments, or logs.
- JSON, CI, redirected-terminal, and `--no-input` use cases fail promptly
  instead of waiting for input. Local-only and offline commands remain
  credential-free.
- Linux amd64 and arm64 binaries are built natively in Debian 12. The same
  verified binaries are packaged as `.deb` files and tested on Debian 12/13
  and Ubuntu 24.04/26.04.
- The release manifest records every archive and Debian package with its size,
  target, and SHA-256 digest.
- A signed APT repository pipeline validates release manifests, publishes
  expiring metadata and by-hash indexes, and rejects altered metadata,
  unsigned repositories, and package checksum mismatches.

This is a prerelease. Native execution is not a sandbox, authenticated Mistral
model access remains account-dependent, and the documented external validation
gates still apply.
