#!/usr/bin/env bash
# Use the same Android toolchain for validation and publishing.
set -euo pipefail

rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked --version 4.1.2
"$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --licenses < <(yes) > /dev/null
