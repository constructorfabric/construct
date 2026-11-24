#!/usr/bin/env bash
# Build the chat core (the `chat-bridge` crate) for the phone apps, once for each target:
#
#   ios      the iPhone device (aarch64-apple-ios) and the iPhone Simulator (aarch64-apple-ios-sim),
#            one static library each
#   android  one shared library for each ABI (arm64-v8a, x86_64), through cargo-ndk
#   all      both (the default)
#
# Everything lands in one folder outside the repository (CONSTRUCT_CORE_BUILD_DIR, default
# ~/Library/Caches/construct-chat-core on a Mac), which the Gradle and Xcode builds read:
#
#   <dir>/ios/device/libconstruct_chat_bridge.a
#   <dir>/ios/simulator/libconstruct_chat_bridge.a
#   <dir>/android/<abi>/libconstruct_chat_bridge.so
#
# The cargo cache is in that folder too (cargo-target), unless CARGO_TARGET_DIR says otherwise. A
# second run with unchanged sources does nothing: cargo finds every target up to date and a library
# is copied only when its bytes differ.
set -euo pipefail

cd "$(dirname "$0")/../.."
root=$(pwd -P)

fail() {
  echo "error: $*" >&2
  exit 1
}

what=${1:-all}
case "$what" in ios | android | all) ;; *) fail "usage: build-core.sh [ios|android|all]" ;; esac

command -v cargo >/dev/null || fail "cargo is not installed (https://rustup.rs)"
build_dir=${CONSTRUCT_CORE_BUILD_DIR:-$HOME/Library/Caches/construct-chat-core}
# A relative folder is taken from the repository root; it is made absolute before the check below,
# so "build" cannot pass as outside the repository.
case "$build_dir" in /*) ;; *) build_dir=$root/$build_dir ;; esac
mkdir -p "$build_dir"
build_dir=$(cd "$build_dir" && pwd -P)
case "$build_dir" in "$root" | "$root"/*) fail "CONSTRUCT_CORE_BUILD_DIR must be outside the repository" ;; esac
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$build_dir/cargo-target}

# A small library: optimise for size, drop the symbols. Cargo builds each profile setting apart, so
# the desktop's build is not touched.
export CARGO_PROFILE_RELEASE_OPT_LEVEL=s
export CARGO_PROFILE_RELEASE_LTO=true
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export CARGO_PROFILE_RELEASE_STRIP=symbols
export CARGO_PROFILE_RELEASE_DEBUG=none

lib=construct_chat_bridge

# The platform versions come from the files that decide them, so the Rust build cannot drift from
# the apps: the Android minimum API level from the version catalog, the iPhone deployment target
# from the Xcode project spec.
android_api=$(sed -n 's/^androidMinSdk = "\([0-9]*\)".*/\1/p' mobile/gradle/libs.versions.toml | head -n 1)
ios_target=$(sed -n 's/^ *iOS: *"\([0-9.]*\)".*/\1/p' mobile/iosApp/project.yml | head -n 1)
[ -n "$android_api" ] || fail "androidMinSdk is not in mobile/gradle/libs.versions.toml"
[ -n "$ios_target" ] || fail "the iOS deployment target is not in mobile/iosApp/project.yml"

# The Gradle tasks (mobile/build.gradle.kts) list the crates this library is built from, and so does
# the workflow's path filter. A new path dependency would leave both stale, so stop when the crates
# the library needs are not the ones listed.
check_crates() {
  local found
  found=$(cargo metadata --locked --format-version 1 | python3 -I -c '
import json, sys
meta = json.load(sys.stdin)
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
packages = {p["id"]: p for p in meta["packages"]}
start = next(i for i, p in packages.items() if p["name"] == "chat-bridge")
seen, todo = set(), [start]
while todo:
    current = todo.pop()
    if current in seen:
        continue
    seen.add(current)
    todo += [d["pkg"] for d in nodes[current]["deps"]]
print(" ".join(sorted(packages[i]["name"] for i in seen if packages[i]["source"] is None)))
')
  [ "$found" = "chat-bridge chat-core connectors phone-pairing" ] ||
    fail "the library is built from: $found; update rustSources in mobile/build.gradle.kts and the paths in .github/workflows/mobile.yml, then this check"
}

need_targets() {
  local installed target
  installed=$(rustup target list --installed 2>/dev/null) || fail "rustup is not installed"
  for target in "$@"; do
    grep -qx "$target" <<<"$installed" || fail "the Rust target $target is missing: rustup target add $target"
  done
}

# Copy a file only when its bytes differ, so an unchanged build leaves the outputs alone.
install_if_changed() {
  mkdir -p "$(dirname "$2")"
  cmp -s "$1" "$2" || cp "$1" "$2"
}

build_ios() {
  command -v xcrun >/dev/null || fail "Xcode is needed for the iPhone libraries"
  need_targets aarch64-apple-ios aarch64-apple-ios-sim
  export IPHONEOS_DEPLOYMENT_TARGET=$ios_target
  local target folder
  for pair in aarch64-apple-ios:device aarch64-apple-ios-sim:simulator; do
    target=${pair%%:*}
    folder=${pair##*:}
    # Only the static library: Kotlin/Native links it into the app.
    cargo rustc --release --locked -p chat-bridge --lib --target "$target" --crate-type staticlib
    install_if_changed "$CARGO_TARGET_DIR/$target/release/lib$lib.a" "$build_dir/ios/$folder/lib$lib.a"
  done
}

build_android() {
  command -v cargo-ndk >/dev/null || fail "cargo-ndk is not installed (cargo install cargo-ndk)"
  need_targets aarch64-linux-android x86_64-linux-android
  if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    local sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
    [ -n "$sdk" ] && [ -d "$sdk/ndk" ] || fail "the Android NDK is not installed (sdkmanager 'ndk;<version>'), or set ANDROID_NDK_HOME"
    ANDROID_NDK_HOME=$(find "$sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)
    export ANDROID_NDK_HOME
  fi
  # cargo-ndk writes its copies to a scratch folder; only a changed library reaches the real one.
  local abi
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' EXIT
  cargo ndk --platform "$android_api" -t arm64-v8a -t x86_64 -o "$scratch" build --release --locked -p chat-bridge --lib
  for abi in arm64-v8a x86_64; do
    install_if_changed "$scratch/$abi/lib$lib.so" "$build_dir/android/$abi/lib$lib.so"
  done
}

check_crates
case "$what" in
  ios) build_ios ;;
  android) build_android ;;
  all)
    build_ios
    build_android
    ;;
esac

echo "The chat core is in $build_dir"
