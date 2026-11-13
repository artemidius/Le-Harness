#!/usr/bin/env bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

package_directory_input=${1:?package directory is required}
package_directory=$(cd "$package_directory_input" && pwd)
package=$(find "$package_directory" -maxdepth 1 -name 'le-harness_*.deb' -print -quit)
architecture=$(dpkg-deb --field "$package" Architecture)
workspace=$(mktemp -d)
trap 'rm -rf "$workspace"' EXIT
chmod 755 "$workspace"
expected_version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')

export GNUPGHOME="$workspace/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --passphrase '' --quick-gen-key \
  'Le Harness APT Test <apt-test@le-harness.invalid>' ed25519 sign 1d
fingerprint=$(gpg --batch --with-colons --fingerprint \
  'Le Harness APT Test' | awk -F: '$1 == "fpr" { print $10; exit }')

repository="$workspace/repository"
python3 scripts/apt/build_repository.py \
  --output "$repository" \
  --suite "preview=$package_directory" \
  --architectures "$architecture" \
  --signing-key "$fingerprint" \
  --valid-days 2 \
  --max-bytes 50000000

test -s "$repository/dists/preview/InRelease"
test -s "$repository/dists/preview/Release.gpg"
test -s "$repository/dists/preview/main/binary-$architecture/Packages.gz"
test -n "$(find "$repository/dists/preview" -path '*/by-hash/SHA256/*' -type f -print -quit)"
grep -Fq 'Acquire-By-Hash: yes' "$repository/dists/preview/Release"
grep -Fq 'Valid-Until:' "$repository/dists/preview/Release"
test "$(cat "$repository/SIGNING-KEY-FINGERPRINT")" = "$fingerprint"
recorded_size=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["repository_bytes"])' "$repository/repository-manifest.json")
actual_size=$(find "$repository" -type f -printf '%s\n' | awk '{ total += $1 } END { print total }')
test "$recorded_size" = "$actual_size"
gpg --batch --verify "$repository/dists/preview/InRelease"
gpg --batch --verify \
  "$repository/dists/preview/Release.gpg" \
  "$repository/dists/preview/Release"

write_source() {
  local root=$1
  cp "$root/le-harness-archive-keyring.gpg" /usr/share/keyrings/le-harness-test.gpg
  cat > /etc/apt/sources.list.d/le-harness-test.sources <<EOF
Types: deb
URIs: file:$root
Suites: preview
Components: main
Architectures: $architecture
Signed-By: /usr/share/keyrings/le-harness-test.gpg
EOF
  rm -rf /var/lib/apt/lists/*
}

write_source "$repository"
apt-get update -qq
apt-get install --yes le-harness > /tmp/apt-valid-install.log
test "$(le-harness --version)" = "le-harness $expected_version"
apt-get purge --yes le-harness > /tmp/apt-valid-purge.log

signature_tamper="$workspace/signature-tamper"
cp -a "$repository" "$signature_tamper"
sed -i 's/Origin: Le Harness/Origin: Not Le Harness/' \
  "$signature_tamper/dists/preview/InRelease"
write_source "$signature_tamper"
if apt-get update -qq; then
  echo 'APT accepted tampered signed metadata' >&2
  exit 1
fi

unsigned="$workspace/unsigned"
cp -a "$repository" "$unsigned"
rm "$unsigned/dists/preview/InRelease" "$unsigned/dists/preview/Release.gpg"
write_source "$unsigned"
if apt-get update -qq; then
  echo 'APT accepted unsigned repository metadata' >&2
  exit 1
fi

package_tamper="$workspace/package-tamper"
cp -a "$repository" "$package_tamper"
tampered_package=$(find "$package_tamper/pool" -name 'le-harness_*.deb' -print -quit)
printf 'tampered' >> "$tampered_package"
write_source "$package_tamper"
apt-get update -qq
rm -f /var/cache/apt/archives/le-harness_*.deb
if apt-get install --yes le-harness; then
  echo 'APT installed a package whose checksum did not match the signed index' >&2
  exit 1
fi
