#!/bin/bash
# Imports the Developer ID certificate into a keychain of its own and prints its identity (the SHA-1 hash,
# not a secret) on stdout. The keychain is $RUNNER_TEMP/signing/signing.keychain-db; the user's own keychains
# are not touched, and scripts/macos-clean-signing.sh removes the new one after the job. codesign finds an
# identity only in a keychain that is in the search list, so the new one is added to it.
# Reads MAC_CERTIFICATE_BASE64 (a .p12), MAC_CERTIFICATE_PASSWORD and APPLE_TEAM_ID from the environment by
# name; never prints them. No `set -x` here.
set -euo pipefail

for name in RUNNER_TEMP MAC_CERTIFICATE_BASE64 MAC_CERTIFICATE_PASSWORD APPLE_TEAM_ID; do
  [ -n "${!name:-}" ] || { echo "$name is not set" >&2; exit 1; }
done

dir="$RUNNER_TEMP/signing"
keychain="$dir/signing.keychain-db"

mkdir -p "$dir"
chmod 700 "$dir"
# A random password for the throw-away keychain, not one of the secrets.
password=$(openssl rand -hex 16)

printf '%s' "$MAC_CERTIFICATE_BASE64" | openssl base64 -d -A -out "$dir/certificate.p12"
security create-keychain -p "$password" "$keychain"
# No auto-lock and no timeout: notarization takes minutes, and the disk image is signed after it.
security set-keychain-settings "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$dir/certificate.p12" -k "$keychain" -P "$MAC_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign -T /usr/bin/security > /dev/null
rm -f "$dir/certificate.p12"
# Lets codesign use the key without a prompt.
security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" > /dev/null

# The new keychain first, then the user's own, as they were.
existing=()
while IFS= read -r line; do
  line=${line//\"/}
  line=${line#"${line%%[![:space:]]*}"}
  [ -n "$line" ] && existing+=("$line")
done < <(security list-keychains -d user)
security list-keychains -d user -s "$keychain" "${existing[@]}"

# Read into a variable first: a pipe to a program that exits early kills the writer (SIGPIPE) under pipefail.
identities=$(security find-identity -v -p codesigning "$keychain")
identity=$(awk -v team="($APPLE_TEAM_ID)" '/Developer ID Application/ && index($0, team) { print $2; exit }' <<< "$identities")
if [ -z "$identity" ]; then
  echo "no Developer ID Application identity of the team in the certificate" >&2
  exit 1
fi
echo "$identity"
