# APT repository signing

Le Harness APT clients trust signed repository metadata, not a detached
signature on an individual `.deb`. Every snapshot contains `Release`,
`InRelease`, `Release.gpg`, SHA-256 package indexes, compressed indexes, and
content-addressed by-hash copies. Release metadata expires after 30 days.

## Key custody

The production archive identity uses an offline certification-only primary key
and a dedicated, expiring signing subkey:

1. Generate the primary key on an offline machine and create a revocation
   certificate immediately.
2. Add a signing-only subkey with a documented expiry and export only that
   secret subkey for the protected release environment.
3. Keep the primary secret key and revocation certificate in separate encrypted
   backups. Neither belongs in the repository, workflow artifacts, logs, or
   ordinary developer keyrings.
4. Publish the complete public key and its full fingerprint with the repository.
   Verify that fingerprint out of band before adding the APT source.
5. Rotate the signing subkey before expiry by publishing metadata signed by both
   old and new trust paths during the transition. If compromised, revoke it,
   publish the revocation, remove the CI secret, and issue a newly reviewed
   repository snapshot.

Pull requests and forks use disposable test keys. Production signing is allowed
only in the protected deployment job after package hashes, suites, and the
release manifest have been validated. The primary private key is never loaded
into CI.

## Snapshot builder

The builder accepts one package directory per suite, refuses nonempty output
directories, validates package names and architectures, and applies a hard size
limit. Example with an isolated signing keyring:

```text
GNUPGHOME=/secure/signing-subkey \
python3 scripts/apt/build_repository.py \
  --output apt-site \
  --suite preview=packages/preview \
  --suite stable=packages/stable \
  --architectures amd64,arm64 \
  --signing-key FULL_FINGERPRINT
```

The generated `repository-manifest.json` records suites, packages, package
hashes, architectures, metadata validity, signing fingerprint, and snapshot
size. Publishing and retention are separate operations so a failed candidate
cannot mutate the last working public snapshot.

The published archive identity has fingerprint
`520A 6749 CEFE 62FC EEA3 0447 2160 9ACA AA5F 447E`. Its certification-only
primary key is kept outside CI. The protected `apt-signing` deployment
environment receives only the expiring signing subkey and its passphrase.

Public snapshots are serialized by the `apt-pages` concurrency group. Metadata
is valid for 30 days and the scheduled workflow refreshes it every week from
the unchanged, manifest-verified release packages. A failed build or signature
check cannot reach the Pages deployment job, leaving the previous snapshot
available. A maintainer can provide a published tag to the manual workflow for
recovery.
