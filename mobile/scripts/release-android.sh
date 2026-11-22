#!/usr/bin/env bash
# Build the signed Android release APK.
#
# The key comes only from the environment: ANDROID_KEYSTORE_PATH, ANDROID_KEY_ALIAS,
# ANDROID_KEYSTORE_PASSWORD and ANDROID_KEY_PASSWORD (optional for a PKCS12 keystore).
#
# While constructApplicationIdIsFinal is false (gradle.properties) the APK is installed on a running
# emulator or device and two screenshots, light and dark, are saved in a folder this script prints.
# Nothing is published. When it is true, the signed APK is published as a GitHub pre-release.
#
# Set RELEASE_DRAFT=1 to make the GitHub release a draft (for trying the flow out).
# A release needs a new constructVersionName (its tag is android-v<name>) and a higher
# constructVersionCode in gradle.properties; the tag points at the pushed HEAD the APK was built from.
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "error: $*" >&2
  exit 1
}

prop() {
  sed -n "s/^$1=//p" gradle.properties | tail -n 1
}

app_id=$(prop constructApplicationId)
is_final=$(prop constructApplicationIdIsFinal)
[ -n "$app_id" ] || fail "constructApplicationId is not set in gradle.properties"
case "$is_final" in true | false) ;; *) fail "constructApplicationIdIsFinal in gradle.properties must be true or false" ;; esac

# An APK from an earlier run must not outlive a failed one.
apk_dir=androidApp/build/outputs/apk/release
rm -rf "$apk_dir"

# Gradle builds an unsigned release APK when the key is missing, so check it here.
for name in ANDROID_KEYSTORE_PATH ANDROID_KEY_ALIAS ANDROID_KEYSTORE_PASSWORD; do
  [ -n "${!name:-}" ] || fail "$name is not set; the release APK is signed from the environment"
done
case "$ANDROID_KEYSTORE_PATH" in /*) ;; *) fail "ANDROID_KEYSTORE_PATH must be an absolute path" ;; esac
[ -f "$ANDROID_KEYSTORE_PATH" ] || fail "the file named by ANDROID_KEYSTORE_PATH does not exist"
export ANDROID_KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-$ANDROID_KEYSTORE_PASSWORD}"

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
[ -n "$sdk" ] || fail "ANDROID_HOME is not set"
apksigner=$(find "$sdk/build-tools" -name apksigner -type f 2>/dev/null | sort -V | tail -n 1)
[ -n "$apksigner" ] || fail "apksigner is not in $sdk/build-tools"
adb="$sdk/platform-tools/adb"

./gradlew --quiet :androidApp:assembleRelease

apk=$(find "$apk_dir" -name '*.apk' | head -n 1)
[ -n "$apk" ] || fail "the build made no APK"
if ! "$apksigner" verify --print-certs "$apk" >/dev/null 2>&1; then
  rm -f "$apk"
  fail "the APK is not signed with a valid signature; it was removed"
fi
echo "Signed APK: $apk"
"$apksigner" verify --print-certs "$apk" | grep -E 'certificate (DN|SHA-256)'

if [ "$is_final" = true ]; then
  command -v gh >/dev/null || fail "gh is needed to publish"
  version=$(prop constructVersionName)
  [ -n "$version" ] || fail "constructVersionName is not set in gradle.properties"
  tag="android-v$version"
  # Each release needs a new version name (its tag) and a higher version code (so installed apps update).
  # Bump both in gradle.properties first.
  if gh release view "$tag" >/dev/null 2>&1 || git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
    fail "the release $tag already exists; raise constructVersionName and constructVersionCode in gradle.properties"
  fi
  # The version code must be higher than that of every earlier release, read from the tagged commits.
  code=$(prop constructVersionCode)
  case "$code" in '' | *[!0-9]*) fail "constructVersionCode in gradle.properties must be a number" ;; esac
  git fetch --quiet --tags origin
  while read -r old_tag; do
    old_code=$(git show "$old_tag:mobile/gradle.properties" 2>/dev/null | sed -n 's/^constructVersionCode=//p')
    if [ -n "$old_code" ] && [ "$code" -le "$old_code" ]; then
      fail "constructVersionCode $code is not higher than $old_code of the release $old_tag; installed apps would not update"
    fi
  done < <(git tag --list 'android-v*')
  # The tag must point at the code the APK was built from.
  [ -z "$(git status --porcelain)" ] || fail "the working tree has changes; commit them before publishing"
  head=$(git rev-parse HEAD)
  git fetch --quiet origin
  git branch -r --contains "$head" | grep -q . || fail "HEAD is not pushed; push it before publishing"
  asset="$(dirname "$apk")/construct-android-$version.apk"
  cp "$apk" "$asset"
  flags=(--prerelease --title "Construct Android $version" --notes "Signed Android build $version.")
  case "${RELEASE_DRAFT:-}" in
    "" | 0 | false | no) ;;
    1 | true | yes) flags+=(--draft) ;;
    *) fail "RELEASE_DRAFT must be 1, true, yes, 0, false or no" ;;
  esac
  gh release create "$tag" "$asset" --target "$head" "${flags[@]}"
  exit 0
fi

# Local mode: install and take screenshots. A failed run must not leave the device forced into a theme.
trap '"$adb" shell cmd uimode night auto >/dev/null 2>&1 || true' EXIT
"$adb" get-state >/dev/null 2>&1 || fail "no emulator or device is connected; start one to take the screenshots"
for _ in $(seq 60); do
  [ "$("$adb" shell getprop sys.boot_completed | tr -d '\r')" = 1 ] && break
  sleep 2
done
size=$("$adb" shell wm size | sed -n 's/.*: \([0-9]*\)x\([0-9]*\).*/\1 \2/p' | tr -d '\r')
tap_x=$((${size% *} / 2))
tap_y=$((${size#* } * 57 / 100))
shots=$(mktemp -d "${TMPDIR:-/tmp}/construct-android-shots.XXXXXX")

# An APK signed with another key cannot be installed over the old one: remove it first (this wipes its data).
"$adb" uninstall "$app_id" >/dev/null 2>&1 || true
"$adb" install "$apk" >/dev/null
launcher=$("$adb" shell cmd package resolve-activity --brief -c android.intent.category.LAUNCHER "$app_id" | tail -n 1 | tr -d '\r')
case "$launcher" in "$app_id"/*) ;; *) fail "the app has no launcher activity" ;; esac
for mode in light dark; do
  "$adb" shell cmd uimode night "$([ "$mode" = dark ] && echo yes || echo no)" >/dev/null
  "$adb" shell am force-stop "$app_id"
  "$adb" shell am start -n "$launcher" >/dev/null
  # Wait for the app's window; a slow emulator may show a "not responding" dialog first: choose Wait.
  seen=false
  for _ in $(seq 60); do
    focus=$("$adb" shell dumpsys window | grep mCurrentFocus || true)
    case "$focus" in
      # The dialog's own title names the package too, so test for it first.
      *"Not Responding"*) "$adb" shell input tap "$tap_x" "$tap_y" ;;
      *"$app_id"*) seen=true; break ;;
    esac
    sleep 1
  done
  [ "$seen" = true ] || fail "the app did not come to the front within 60 seconds; no screenshot was saved for the $mode theme"
  sleep 8
  "$adb" exec-out screencap -p >"$shots/$mode.png"
done
echo "Screenshots: $shots"
