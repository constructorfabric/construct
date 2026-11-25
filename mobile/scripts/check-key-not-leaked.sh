#!/usr/bin/env bash
# Save a recognizable fake API key through the app, then search the app's files and the device log for it.
#
#   scripts/check-key-not-leaked.sh android   needs a running emulator or device (one only)
#   scripts/check-key-not-leaked.sh ios       needs Xcode, an iOS Simulator runtime, and xcodegen
#
# The iPhone check runs in the Simulator named "Construct Test", which the iPhone test scripts share
# (scripts/shared_simulator.py): it is made the first time, reset before the run, and shut down at the end.
#
# The key is made up and is not a credential. The script first plants it on purpose in a
# file of its own and checks that the search finds it there, so a search that finds nothing is not a
# broken search. It then fails when the key is in any file of the app or in the log, and says where.
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "error: $*" >&2
  exit 1
}

platform=${1:-}

search_android() (
  local app_id
  local key="sk-android-leak-search-0123456789-do-not-use"
  local product_id
  product_id=$(sed -n 's/^constructApplicationId=//p' gradle.properties | tail -n 1)
  [ -n "$product_id" ] || fail "constructApplicationId is missing"
  app_id="$product_id.leakcheck"
  [ "$(adb devices | grep -c 'device$')" = 1 ] || fail "start exactly one emulator or device"

  # The fixture app has its own data directory, even on a device running the normal app.
  leak_app_id="$app_id"
  trap 'adb uninstall "$leak_app_id.test" >/dev/null 2>&1 || true; adb uninstall "$leak_app_id" >/dev/null 2>&1 || true' EXIT
  ./gradlew -PconstructApplicationId="$app_id" :androidApp:installDebug :androidApp:installDebugAndroidTest --console=plain >/dev/null
  adb shell pm clear "$app_id" >/dev/null
  local run
  run=$(adb shell am instrument -w \
    -e class cf.construct.mobile.android.ModelSetupTest#leavesAFakeKeyForTheLeakSearch \
    -e leakSearch 1 "$app_id.test/androidx.test.runner.AndroidJUnitRunner" | tr -d '\r')
  echo "$run" | grep -q '^OK (1 test)' || fail "the test that saves the key did not pass: $run"

  local data_files
  data_files=$(adb shell run-as "$app_id" find . -type f | tr -d '\r')
  echo "app files after saving the key:"
  echo "$data_files" | sed 's/^/  /'
  echo "$data_files" | grep -q 'no_backup/model_provider.bin$' || fail "the encrypted provider file is not where it should be"

  # The search must find a key that is there.
  adb shell "run-as $app_id sh -c \"echo $key > files/leak-control.txt\""
  adb shell run-as "$app_id" grep -rlF "$key" . | grep -q leak-control.txt || fail "the search does not find a planted key"
  adb shell run-as "$app_id" rm files/leak-control.txt

  local found=0
  local hits
  hits=$(adb shell run-as "$app_id" grep -rlF "$key" . | tr -d '\r' || true)
  [ -z "$hits" ] || { echo "KEY FOUND in the app's files: $hits"; found=1; }
  hits=$(adb shell "grep -rlF '$key' /sdcard/Android/data/$app_id 2>/dev/null" | tr -d '\r' || true)
  [ -z "$hits" ] || { echo "KEY FOUND on shared storage: $hits"; found=1; }
  hits=$(adb logcat -d | awk -v key="$key" 'index($0, key) { found = 1 } END { print found + 0 }')
  if [ "$hits" != 0 ]; then
    echo "KEY FOUND in the log"
    found=1
  fi

  [ "$found" = 0 ] || exit 1
  echo "android: the key is in no file of the app and not in the log"
)

# Runs inside the shared Simulator, which is booted and as new; its id is in SIMULATOR_UDID.
search_ios_in_simulator() (
  local bundle_id run_log key leak_build run_started
  # Passing a key in the test runner's environment causes the simulator to log it.
  key="sk-ios-leak-search-0123456789-do-not-use"
  [ -n "${SIMULATOR_UDID:-}" ] || fail "run this through 'check-key-not-leaked.sh ios'"
  run_log=$(mktemp)
  bundle_id=$(sed -n 's/^ *APP_BUNDLE_ID: *//p' iosApp/project.yml | head -n 1)
  [ -n "$bundle_id" ] || fail "APP_BUNDLE_ID is missing"
  leak_build=$(mktemp -d)
  trap 'rm -rf "$leak_build"' EXIT
  # The log of the Simulator holds the runs before this one: look only at what this run wrote.
  run_started=$(date '+%Y-%m-%d %H:%M:%S')
  # An unsigned Simulator build has no Keychain; an ad-hoc signature gives it one.
  (cd iosApp && xcodegen generate >/dev/null &&
    TEST_RUNNER_LEAK_SEARCH=1 xcodebuild -project Construct.xcodeproj -scheme Construct \
      -destination "platform=iOS Simulator,id=$SIMULATOR_UDID" -derivedDataPath "$leak_build" \
      -only-testing:ConstructTests/ModelSettingsStoreTests/testLeavesAFakeKeyForTheLeakSearch \
      CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- test >"$run_log" 2>&1) || true
  grep -q 'TEST SUCCEEDED' "$run_log" || fail "the test that saves the key did not pass (see $run_log)"

  local data
  data=$(xcrun simctl get_app_container "$SIMULATOR_UDID" "$bundle_id" data)
  echo "app files after saving the key:"
  (cd "$data" && find . -type f | sed 's/^/  /')

  # The search must find a key that is there.
  echo "$key" >"$data/Documents/leak-control.txt"
  grep -rlF "$key" "$data" | grep -q leak-control.txt || fail "the search does not find a planted key"
  rm "$data/Documents/leak-control.txt"

  local found=0
  local hits
  hits=$(grep -rlF "$key" "$data" || true)
  [ -z "$hits" ] || { echo "KEY FOUND in the app's files: $hits"; found=1; }
  hits=$(xcrun simctl spawn "$SIMULATOR_UDID" log show --start "$run_started" --style compact |
    awk -v key="$key" 'index($0, key) { found = 1 } END { print found + 0 }')
  if [ "$hits" != 0 ]; then
    echo "KEY FOUND in the log"
    found=1
  fi
  rm -f "$run_log"

  [ "$found" = 0 ] || exit 1
  echo "ios: the key is in no file of the app and not in the log"
)

search_ios() {
  local bundle_id device_type runtime
  bundle_id=$(sed -n 's/^ *APP_BUNDLE_ID: *//p' iosApp/project.yml | head -n 1)
  [ -n "$bundle_id" ] || fail "APP_BUNDLE_ID is missing"
  device_type=$(xcrun simctl list -j devicetypes | jq -er --arg name "${SIMULATOR_NAME:-iPhone 17}" \
    '.devicetypes[] | select(.name == $name) | .identifier')
  runtime=$(xcrun simctl list -j runtimes | jq -er \
    '[.runtimes[] | select(.isAvailable and (.identifier | contains(".iOS-"))) | .identifier] | last')
  # The helper takes its turn, boots the Simulator, resets it, and shuts it down when this script ends.
  python3 scripts/shared_simulator.py --device-type "$device_type" --runtime "$runtime" --bundle-id "$bundle_id" \
    -- scripts/check-key-not-leaked.sh ios-in-simulator
}

case "$platform" in
android) search_android ;;
ios) search_ios ;;
ios-in-simulator) search_ios_in_simulator ;;
*) fail "usage: check-key-not-leaked.sh android|ios" ;;
esac
