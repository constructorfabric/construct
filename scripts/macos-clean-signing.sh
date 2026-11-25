#!/bin/bash
# Removes what macos-import-certificate.sh left on the runner; runs in a step that always runs, so also after
# a failure.
dir="${RUNNER_TEMP:-}/signing"
keychain="$dir/signing.keychain-db"
[ -n "${RUNNER_TEMP:-}" ] && [ -d "$dir" ] || exit 0
# Only this job's keychain leaves the search list: the list is read again, not restored from a copy.
remaining=()
while IFS= read -r line; do
  line=${line//\"/}
  line=${line#"${line%%[![:space:]]*}"}
  [ -n "$line" ] && [ "$line" != "$keychain" ] && remaining+=("$line")
done < <(security list-keychains -d user)
if [ "${#remaining[@]}" -gt 0 ]; then
  security list-keychains -d user -s "${remaining[@]}" || true
fi
security delete-keychain "$keychain" 2> /dev/null || true
rm -rf "$dir"
