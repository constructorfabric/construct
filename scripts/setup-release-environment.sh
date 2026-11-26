#!/usr/bin/env bash
# Creates the `release` environment of the repository and puts the credentials of the signed releases in it:
# the iPhone build for TestFlight, the Android APK and the macOS disk image. mobile/RELEASING.md says what each
# credential is and where it is used.
#
# Run it as an administrator of the repository (`gh auth status`). It can be run again at any time, in any
# environment: the environment and its secrets end up as the variables below say. A release whose variables
# are all unset is skipped; a release with only some of them set stops the script before anything is changed.
#
#   iPhone   APP_STORE_CONNECT_KEY_PATH (the .p8 file), APP_STORE_CONNECT_KEY_ID, APP_STORE_CONNECT_ISSUER_ID,
#            TESTFLIGHT_INTERNAL_GROUP (the exact name of the internal group)
#   Android  ANDROID_KEYSTORE_PATH (the keystore file), ANDROID_KEY_ALIAS, ANDROID_KEYSTORE_PASSWORD,
#            and ANDROID_KEY_PASSWORD when the key has a password of its own
#   desktop  MAC_CERTIFICATE_PATH (the Developer ID Application .p12 file), MAC_CERTIFICATE_PASSWORD, APPLE_ID,
#            APPLE_APP_SPECIFIC_PASSWORD, APPLE_TEAM_ID
#
# REPO names the repository (default constructorfabric/construct). DRY_RUN=1 checks the inputs and says what it
# would set, and changes nothing. The values go straight to GitHub; nothing is written to a file or printed.
set -euo pipefail

repo=${REPO:-constructorfabric/construct}
dry=${DRY_RUN:-0}
environment=release

fail() {
  echo "error: $*" >&2
  exit 1
}
for tool in gh openssl base64; do
  command -v "$tool" >/dev/null || fail "$tool is not installed"
done
if [ "$dry" != 1 ]; then
  [ "$(gh api "repos/$repo" --jq '.permissions.admin')" = true ] \
    || fail "the logged-in gh account is not an administrator of $repo"
fi

# True when every variable named is set, false when none is; the script stops when only some are.
complete() {
  local name set=0 total=0
  for name in "$@"; do
    total=$((total + 1))
    if [ -n "${!name:-}" ]; then set=$((set + 1)); fi
  done
  [ "$set" -eq "$total" ] && return 0
  [ "$set" -eq 0 ] && return 1
  fail "set all of $* or none of them"
}
readable() { [ -r "$1" ] || fail "$1 is not a readable file"; }
encoded() { base64 <"$1" | tr -d '\n'; }

planned=()
secret() { planned+=("secret:$1:$2"); }

if complete APP_STORE_CONNECT_KEY_PATH APP_STORE_CONNECT_KEY_ID APP_STORE_CONNECT_ISSUER_ID TESTFLIGHT_INTERNAL_GROUP; then
  readable "$APP_STORE_CONNECT_KEY_PATH"
  openssl pkey -in "$APP_STORE_CONNECT_KEY_PATH" -noout 2>/dev/null \
    || fail "APP_STORE_CONNECT_KEY_PATH is not a private key file"
  secret APP_STORE_CONNECT_KEY_P8_BASE64 "$(encoded "$APP_STORE_CONNECT_KEY_PATH")"
  secret APP_STORE_CONNECT_KEY_ID "$APP_STORE_CONNECT_KEY_ID"
  secret APP_STORE_CONNECT_ISSUER_ID "$APP_STORE_CONNECT_ISSUER_ID"
  planned+=("variable:TESTFLIGHT_INTERNAL_GROUP:$TESTFLIGHT_INTERNAL_GROUP")
fi

if complete ANDROID_KEYSTORE_PATH ANDROID_KEY_ALIAS ANDROID_KEYSTORE_PASSWORD; then
  readable "$ANDROID_KEYSTORE_PATH"
  secret ANDROID_KEYSTORE_BASE64 "$(encoded "$ANDROID_KEYSTORE_PATH")"
  secret ANDROID_KEY_ALIAS "$ANDROID_KEY_ALIAS"
  secret ANDROID_KEYSTORE_PASSWORD "$ANDROID_KEYSTORE_PASSWORD"
  if [ -n "${ANDROID_KEY_PASSWORD:-}" ]; then secret ANDROID_KEY_PASSWORD "$ANDROID_KEY_PASSWORD"; fi
fi

if complete MAC_CERTIFICATE_PATH MAC_CERTIFICATE_PASSWORD APPLE_ID APPLE_APP_SPECIFIC_PASSWORD APPLE_TEAM_ID; then
  readable "$MAC_CERTIFICATE_PATH"
  subject=
  for legacy in "" "-legacy"; do
    # shellcheck disable=SC2086 # the empty option must vanish
    subject=$(openssl pkcs12 $legacy -in "$MAC_CERTIFICATE_PATH" -passin env:MAC_CERTIFICATE_PASSWORD \
      -nokeys -clcerts 2>/dev/null | openssl x509 -noout -subject 2>/dev/null) && break
  done
  [ -n "$subject" ] || fail "MAC_CERTIFICATE_PATH does not open with MAC_CERTIFICATE_PASSWORD"
  grep -q 'Developer ID Application' <<<"$subject" \
    || fail "the certificate is not a Developer ID Application certificate: $subject"
  secret MAC_CERTIFICATE_BASE64 "$(encoded "$MAC_CERTIFICATE_PATH")"
  secret MAC_CERTIFICATE_PASSWORD "$MAC_CERTIFICATE_PASSWORD"
  secret APPLE_ID "$APPLE_ID"
  secret APPLE_APP_SPECIFIC_PASSWORD "$APPLE_APP_SPECIFIC_PASSWORD"
  secret APPLE_TEAM_ID "$APPLE_TEAM_ID"
fi

[ "${#planned[@]}" -gt 0 ] || fail "no credentials given: see the variables at the top of this script"

if [ "$dry" = 1 ]; then
  echo "Dry run: the environment $environment of $repo would get, from main and v* tags only:"
  for item in "${planned[@]}"; do
    kind=${item%%:*}
    rest=${item#*:}
    echo "  $kind ${rest%%:*}"
  done
  exit 0
fi

# The environment holds the credentials and allows deployments from main and v* tags only.
echo '{"wait_timer": 0, "prevent_self_review": false, "reviewers": [],
  "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}' \
  | gh api -X PUT "repos/$repo/environments/$environment" --input - >/dev/null
existing=$(gh api --paginate "repos/$repo/environments/$environment/deployment-branch-policies" \
  --jq '.branch_policies[] | "\(.type):\(.name)"')
for wanted in branch:main 'tag:v*'; do
  if ! grep -qxF "$wanted" <<<"$existing"; then
    gh api -X POST "repos/$repo/environments/$environment/deployment-branch-policies" \
      -f "name=${wanted#*:}" -f "type=${wanted%%:*}" >/dev/null
  fi
done

for item in "${planned[@]}"; do
  kind=${item%%:*}
  rest=${item#*:}
  name=${rest%%:*}
  value=${rest#*:}
  if [ "$kind" = secret ]; then
    printf '%s' "$value" | gh secret set "$name" --env "$environment" --repo "$repo"
  else
    gh variable set "$name" --env "$environment" --repo "$repo" --body "$value"
  fi
done
echo "Done: the environment $environment of $repo has ${#planned[@]} entries; list them with: gh secret list --env $environment --repo $repo"
