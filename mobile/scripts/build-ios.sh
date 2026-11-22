#!/usr/bin/env bash
# Build the iPhone app.
#
# While APP_BUNDLE_ID_IS_FINAL is "NO" (iosApp/project.yml) the app is built for the Simulator,
# installed and launched, and two screenshots, light and dark, are saved in a folder this script
# prints. Nothing is sent to Apple and no key is read. A final id is not handled yet: the script
# stops.
#
# IOS_SIMULATOR names the simulator to use (default: the first single-screen iPhone that is
# available; the two-screen "Duo" needs a display chosen for each screenshot).
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "error: $*" >&2
  exit 1
}

spec=iosApp/project.yml
prop() {
  sed -n "s/^ *$1: *//p" "$spec" | head -n 1 | tr -d '"'
}

bundle_id=$(prop APP_BUNDLE_ID)
is_final=$(prop APP_BUNDLE_ID_IS_FINAL)
[ -n "$bundle_id" ] || fail "APP_BUNDLE_ID is not set in $spec"
case "$is_final" in YES | NO) ;; *) fail "APP_BUNDLE_ID_IS_FINAL in $spec must be YES or NO" ;; esac
[ "$is_final" = NO ] || fail "the bundle id is final, and building for a device is not supported by this script yet"

case "$(xcode-select -p 2>/dev/null)" in
  */Contents/Developer) ;;
  *) fail "xcode-select must point at the Xcode app (sudo xcode-select -s /Applications/Xcode.app/Contents/Developer), not the Command Line Tools" ;;
esac
command -v xcodegen >/dev/null || fail "xcodegen is not installed (brew install xcodegen)"

# The Kotlin framework is built by Gradle from the Xcode build; it needs a JDK 17.
if [ -z "${JAVA_HOME:-}" ]; then
  JAVA_HOME=$(/usr/libexec/java_home -v 17 2>/dev/null) || fail "JAVA_HOME is not set and no JDK 17 was found"
  export JAVA_HOME
fi

# Pick the simulator: the one named, or the first available iPhone.
devices=$(xcrun simctl list devices available)
if [ -n "${IOS_SIMULATOR:-}" ]; then
  line=$(printf '%s\n' "$devices" | grep -F "    $IOS_SIMULATOR (" | head -n 1 || true)
else
  line=$(printf '%s\n' "$devices" | grep -E '^ +iPhone ' | grep -v ' Duo ' | head -n 1 || true)
fi
[ -n "$line" ] || fail "no matching iPhone simulator is available (xcrun simctl list devices available)"
udid=$(printf '%s\n' "$line" | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')
name=$(printf '%s\n' "$line" | sed -E 's/^ +(.*) \([0-9A-F-]{36}\).*/\1/')
echo "Simulator: $name"

shots=$(mktemp -d "${TMPDIR:-/tmp}/construct-ios-shots.XXXXXX")
booted_here=false
cleanup() {
  xcrun simctl ui "$udid" appearance light >/dev/null 2>&1 || true
  if [ "$booted_here" = true ]; then
    xcrun simctl shutdown "$udid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if ! printf '%s\n' "$line" | grep -q '(Booted)'; then
  xcrun simctl boot "$udid"
  booted_here=true
fi
xcrun simctl bootstatus "$udid" -b >/dev/null

(cd iosApp && xcodegen generate --quiet)
derived=build/ios
xcodebuild -project iosApp/Construct.xcodeproj -scheme Construct \
  -destination "id=$udid" -derivedDataPath "$derived" -quiet build
app="$derived/Build/Products/Debug-iphonesimulator/Construct.app"
[ -d "$app" ] || fail "the build made no app"

xcrun simctl uninstall "$udid" "$bundle_id" >/dev/null 2>&1 || true
xcrun simctl install "$udid" "$app"
for mode in light dark; do
  xcrun simctl ui "$udid" appearance "$mode"
  xcrun simctl terminate "$udid" "$bundle_id" >/dev/null 2>&1 || true
  xcrun simctl launch "$udid" "$bundle_id" >/dev/null
  # The first frames are the launch screen while the Kotlin framework loads: take a screenshot every
  # second and keep the one that three captures in a row agree on, after at least three seconds.
  same=0
  rm -f "$shots/previous.png"
  for second in $(seq 60); do
    sleep 1
    xcrun simctl io "$udid" screenshot "$shots/$mode.png" >/dev/null 2>&1 || continue
    if [ -s "$shots/previous.png" ] && cmp -s "$shots/previous.png" "$shots/$mode.png"; then
      same=$((same + 1))
    else
      same=0
    fi
    cp "$shots/$mode.png" "$shots/previous.png"
    [ "$second" -ge 3 ] && [ "$same" -ge 2 ] && break
  done
  rm -f "$shots/previous.png"
  [ "$same" -ge 2 ] || fail "the screen did not settle within 60 seconds; no screenshot was saved for the $mode theme"
done
echo "Screenshots: $shots"
