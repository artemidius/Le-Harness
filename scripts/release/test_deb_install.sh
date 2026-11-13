#!/usr/bin/env bash
set -euo pipefail

package_directory_input=${1:?package directory is required}
expected_upstream=${2:?upstream version is required}
expected_architecture=${3:?Debian architecture is required}
package_directory=$(cd "$package_directory_input" && pwd)

main_package=$(find "$package_directory" -maxdepth 1 -type f -name 'le-harness_*.deb' -print -quit)
upgrade_fixture=$(find "$package_directory" -maxdepth 1 -type f -name 'upgrade-fixture-*.deb' -print -quit)
test -n "$main_package"
test -n "$upgrade_fixture"

main_version=$(dpkg-deb --field "$main_package" Version)
fixture_version=$(dpkg-deb --field "$upgrade_fixture" Version)
test "$(dpkg-deb --field "$main_package" Architecture)" = "$expected_architecture"
dpkg --compare-versions "$fixture_version" lt "$main_version"

case "$expected_upstream" in
  *-*) expected_debian="${expected_upstream%%-*}~${expected_upstream#*-}-1" ;;
  *) expected_debian="$expected_upstream-1" ;;
esac
test "$main_version" = "$expected_debian"

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install --yes "$upgrade_fixture" > /tmp/install-fixture.log
test "$(dpkg-query --show --showformat='${Version}' le-harness)" = "$fixture_version"
test "$(le-harness --version)" = "le-harness $expected_upstream"

state=/root/.local/state/le-harness/sessions/user-owned.jsonl
mkdir -p "$(dirname "$state")"
printf '%s\n' '{"owned_by":"user"}' > "$state"

apt-get install --yes "$main_package" > /tmp/install-main.log
test "$(dpkg-query --show --showformat='${Version}' le-harness)" = "$main_version"
le-harness --no-input doctor > /tmp/doctor.log

apt-get remove --yes le-harness > /tmp/remove.log
test -f "$state"
hash -r
if command -v le-harness >/dev/null; then
  echo "le-harness remained after package removal" >&2
  exit 1
fi

apt-get install --yes "$main_package" > /tmp/reinstall-main.log
apt-get purge --yes le-harness > /tmp/purge.log
test -f "$state"

if grep -R -F "Mistral API key (hidden" \
  /tmp/install-*.log /tmp/reinstall-main.log /tmp/remove.log /tmp/purge.log; then
  echo "package operation unexpectedly prompted for a credential" >&2
  exit 1
fi
