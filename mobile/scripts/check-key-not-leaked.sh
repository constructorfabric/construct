#!/usr/bin/env bash
# Save a recognizable fake API key through the app, then search the app's files and the device log for it.
#
#   scripts/check-key-not-leaked.sh android   needs a running emulator or device (one only)
#   scripts/check-key-not-leaked.sh ios       needs Xcode, a booted iPhone Simulator, and xcodegen
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

search_android() {
  local app_id
  local key="sk-android-leak-search-0123456789-do-not-use"
  app_id=$(sed -n 's/^constructApplicationId=//p' gradle.properties | tail -n 1)
  [ "$(adb devices | grep -c 'device$')" = 1 ] || fail "start exactly one emulator or device"

  # The apps are installed and the test is run by hand, because the Gradle task removes the app after the
  # run and its data with it.
  ./gradlew :androidApp:installDebug :androidApp:installDebugAndroidTest --console=plain >/dev/null
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
  echo "$data_files" | grep -q 'no_backup/model_key.bin$' || fail "the encrypted key file is not where it should be"

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
  local log_file
  log_file=$(mktemp)
  adb logcat -d >"$log_file"
  grep -qF "$key" "$log_file" && { echo "KEY FOUND in the log"; found=1; }
  rm -f "$log_file"

  adb shell pm clear "$app_id" >/dev/null
  [ "$found" = 0 ] || exit 1
  echo "android: the key is in no file of the app and not in the log"
}

search_ios() {
  local bundle_id device run_log key
  # Passing a key in the test runner's environment causes the simulator to log it.
  key="sk-ios-leak-search-0123456789-do-not-use"
  run_log=$(mktemp)
  bundle_id=$(sed -n 's/^ *APP_BUNDLE_ID: *//p' iosApp/project.yml | head -n 1)
  device=${SIMULATOR_NAME:-iPhone 17}
  xcrun simctl boot "$device" 2>/dev/null || true
  xcrun simctl bootstatus "$device" >/dev/null
  # An unsigned Simulator build has no Keychain; an ad-hoc signature gives it one.
  (cd iosApp && xcodegen generate >/dev/null &&
    TEST_RUNNER_LEAK_SEARCH=1 xcodebuild -project Construct.xcodeproj -scheme Construct \
      -destination "platform=iOS Simulator,name=$device" -derivedDataPath "${TMPDIR:-/tmp}/construct-leak-search" \
      -only-testing:ConstructTests/ModelSettingsStoreTests/testLeavesAFakeKeyForTheLeakSearch \
      CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- test >"$run_log" 2>&1) || true
  grep -q 'TEST SUCCEEDED' "$run_log" || fail "the test that saves the key did not pass (see $run_log)"

  local data
  data=$(xcrun simctl get_app_container "$device" "$bundle_id" data)
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
  local log_file
  log_file=$(mktemp)
  xcrun simctl spawn "$device" log show --last 10m --style compact >"$log_file" 2>/dev/null
  grep -qF "$key" "$log_file" && { echo "KEY FOUND in the log"; found=1; }
  rm -f "$log_file" "$run_log"

  xcrun simctl uninstall "$device" "$bundle_id" || true
  [ "$found" = 0 ] || exit 1
  echo "ios: the key is in no file of the app and not in the log"
}

case "$platform" in
android) search_android ;;
ios) search_ios ;;
*) fail "usage: check-key-not-leaked.sh android|ios" ;;
esac
