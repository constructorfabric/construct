#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "error: $*" >&2
  exit 1
}

[ "${GITHUB_EVENT_NAME:-}" = workflow_dispatch ] &&
  [ "${RELEASE_CONFIRMED:-}" = true ] || fail "dispatch the release explicitly and confirm it"
jq -e --arg repo "${GITHUB_REPOSITORY:-}" \
  'type == "array" and any(.[]; type == "string" and ascii_downcase == ($repo | ascii_downcase))' >/dev/null \
  <<< "${RELEASE_REPOSITORIES:-[\"constructorfabric/construct\"]}" &&
  [ "${GITHUB_REF:-}" = refs/heads/main ] || fail "a release requires canonical main"
[[ "${RELEASE_SOURCE_SHA:-}" =~ ^[0-9a-f]{40}$ ]] &&
  [ "$RELEASE_SOURCE_SHA" = "${GITHUB_SHA:-}" ] &&
  [ "$RELEASE_SOURCE_SHA" = "$(git rev-parse HEAD)" ] || fail "the release SHA must match the workflow and checkout"
checkout_status=$(git status --porcelain=v1 --untracked-files=all)
[ -z "$checkout_status" ] || fail "a release requires a clean checkout"

canonical_ref=$(git ls-remote --exit-code "https://github.com/$GITHUB_REPOSITORY.git" refs/heads/main) ||
  fail "could not read main of $GITHUB_REPOSITORY"
[ "${canonical_ref%%$'\t'*}" = "$RELEASE_SOURCE_SHA" ] || fail "the release SHA is no longer canonical main"

if [ "$#" -eq 0 ]; then
  exit 0
fi
[ "$#" -eq 1 ] || fail "choose one release platform"
case "$1" in
  ios)
    grep -Eq '^[[:space:]]*APP_BUNDLE_ID_IS_FINAL: "YES"$' mobile/iosApp/project.yml \
      || fail "commit the final iOS bundle ID before releasing"
    ;;
  android)
    grep -qx 'constructApplicationIdIsFinal=true' mobile/gradle.properties \
      || fail "commit the final Android application ID before releasing"
    ;;
  *) fail "choose ios or android" ;;
esac
