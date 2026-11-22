#!/usr/bin/env bash
# Build the iPhone app.
#
# While APP_BUNDLE_ID_IS_FINAL is "NO" (iosApp/project.yml) the app is built for the Simulator,
# installed and launched, and two screenshots, light and dark, are saved in a folder this script
# prints. Nothing is sent to Apple and no key is read.
#
# When it is "YES" the app is archived for a device and the build is sent to TestFlight. The upload
# uses the App Store Connect API key when APP_STORE_CONNECT_KEY_PATH, APP_STORE_CONNECT_KEY_ID and
# APP_STORE_CONNECT_ISSUER_ID are set, and otherwise the Apple account Xcode is signed in with. The
# build number is <YYMMDD>.<count of the day>; the script writes it into the spec after the upload.
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

case "$(xcode-select -p 2>/dev/null)" in
  */Contents/Developer) ;;
  *) fail "xcode-select must point at the Xcode app (sudo xcode-select -s /Applications/Xcode.app/Contents/Developer), not the Command Line Tools" ;;
esac
command -v xcodegen >/dev/null || fail "xcodegen is not installed (brew install xcodegen)"

# The Kotlin framework is built by Gradle from the Xcode build; it needs a JDK 17.
# java_home does not list a Homebrew JDK that is not linked into /Library/Java, so look there too.
if [ -z "${JAVA_HOME:-}" ]; then
  if ! JAVA_HOME=$(/usr/libexec/java_home -v 17 2>/dev/null); then
    JAVA_HOME="$(brew --prefix openjdk@17 2>/dev/null)/libexec/openjdk.jdk/Contents/Home"
    [ -x "$JAVA_HOME/bin/java" ] || fail "JAVA_HOME is not set and no JDK 17 was found"
  fi
  export JAVA_HOME
fi

# Archive for a device and send the build to TestFlight.
upload() {
  local team auth key_vars name set_count today current build version out
  team=$(prop DEVELOPMENT_TEAM)
  [ -n "$team" ] || fail "DEVELOPMENT_TEAM is not set in $spec"

  # The key values go to xcodebuild only; they are never printed.
  auth=(-allowProvisioningUpdates)
  key_vars=(APP_STORE_CONNECT_KEY_PATH APP_STORE_CONNECT_KEY_ID APP_STORE_CONNECT_ISSUER_ID)
  set_count=0
  for name in "${key_vars[@]}"; do
    if [ -n "${!name:-}" ]; then set_count=$((set_count + 1)); fi
  done
  if [ "$set_count" -eq 3 ]; then
    [ -r "$APP_STORE_CONNECT_KEY_PATH" ] || fail "APP_STORE_CONNECT_KEY_PATH names no readable file"
    auth+=(-authenticationKeyPath "$APP_STORE_CONNECT_KEY_PATH"
      -authenticationKeyID "$APP_STORE_CONNECT_KEY_ID"
      -authenticationKeyIssuerID "$APP_STORE_CONNECT_ISSUER_ID")
    echo "Upload: with the App Store Connect API key"
  elif [ "$set_count" -gt 0 ]; then
    fail "set all of ${key_vars[*]}, or none to upload with the Apple account Xcode is signed in with"
  elif defaults read com.apple.dt.Xcode DVTDeveloperAccountManagerAppleIDLists 2>/dev/null | grep -q 'identifier ='; then
    echo "Upload: with the Apple account Xcode is signed in with"
  else
    fail "no App Store Connect API key (${key_vars[*]}) and no Apple account in Xcode (Xcode, Settings, Accounts)"
  fi

  # The build number is <UTC day as YYMMDD>.<count of the day>: the number in the spec plus one on the
  # same day, the first of the day otherwise. It is written back after the upload.
  today=$(date -u +%y%m%d)
  current=$(prop CURRENT_PROJECT_VERSION)
  if [[ "$current" =~ ^$today\.([0-9]+)$ ]]; then
    build="$today.$((BASH_REMATCH[1] + 1))"
  else
    build="$today.1"
  fi
  version=$(prop MARKETING_VERSION)

  out=build/ios-release
  rm -rf "$out"
  mkdir -p "$out"
  (cd iosApp && xcodegen generate --quiet)
  echo "Archive: $version ($build)"
  xcodebuild -project iosApp/Construct.xcodeproj -scheme Construct -configuration Release \
    -destination generic/platform=iOS -derivedDataPath build/ios-device \
    -archivePath "$out/Construct.xcarchive" CURRENT_PROJECT_VERSION="$build" \
    "${auth[@]}" -quiet archive

  cat >"$out/ExportOptions.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>upload</string>
  <key>teamID</key><string>$team</string>
  <key>signingStyle</key><string>automatic</string>
</dict>
</plist>
EOF
  echo "Upload: $bundle_id $version ($build)"
  xcodebuild -exportArchive -archivePath "$out/Construct.xcarchive" \
    -exportOptionsPlist "$out/ExportOptions.plist" -exportPath "$out/export" \
    "${auth[@]}" -quiet
  sed -i '' -E "s/^( *CURRENT_PROJECT_VERSION: ).*/\1\"$build\"/" "$spec"
  echo "Sent $version ($build) to App Store Connect. Processing takes some minutes; the build then"
  echo "shows in TestFlight for the internal group."
}

if [ "$is_final" = YES ]; then
  upload
  exit 0
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
