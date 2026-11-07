# Debian package maintenance

Le Harness Debian packages are produced by `cargo-deb` 3.8.0 from the release
binary already built in the Debian 12 release job. Packaging uses `--no-build`
and `--no-strip`; the workflow extracts the installed payload and verifies that
its binary is byte-identical to the release build.

Cargo prerelease versions follow Debian ordering automatically:

```text
0.1.0-rc.2 -> 0.1.0~rc.2-1
0.1.0-rc.3 -> 0.1.0~rc.3-1
0.1.0      -> 0.1.0-1
```

The package contains `/usr/bin/le-harness` and project documentation under
`/usr/share/doc/le-harness/`. Runtime dependencies are derived from the actual
ELF binary, with Git and the CA certificate bundle declared for repository and
HTTPS workflows. Docker is a suggestion, not a required dependency. There are
no maintainer scripts, services, configuration files, or credential prompts.

The release workflow runs `dpkg-deb` inspection, Lintian error checks, payload
comparison, and version-order assertions. It then installs and upgrades the
package on clean Debian 12, Debian 13, Ubuntu 24.04, and Ubuntu 26.04 systems on
both amd64 and arm64. Removal and purge checks confirm that user-owned state is
not removed.

For a local native build after compiling the release binary:

```text
cargo install cargo-deb --version 3.8.0 --locked
cargo deb --no-build --no-strip --target "$(rustc -vV | sed -n 's/^host: //p')"
```
