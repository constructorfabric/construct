#!/usr/bin/env bash
# Install the same toolchain for unsigned builds and protected releases.
set -euo pipefail

xcode=$(ls -d /Applications/Xcode_*.app | grep -viE 'beta|rc|preview' | sort -V | tail -n 1)
sudo xcode-select -s "$xcode"
xcodebuild -version

cd "${RUNNER_TEMP:?this script runs on a macOS Actions runner}"
curl -fsSL --retry 3 --max-time 120 -o xcodegen.zip \
  https://github.com/yonaskolb/XcodeGen/releases/download/2.48.0/xcodegen.zip
curl -fsSL --retry 3 --max-time 120 -o swiftlint.zip \
  https://github.com/realm/SwiftLint/releases/download/0.65.1/portable_swiftlint.zip
shasum -a 256 --check <<'EOF'
620c9f0dc10849f94728ab5df52d225ebab8187439dd6fee1925e59e80dff892  xcodegen.zip
c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0  swiftlint.zip
EOF
# XcodeGen reads its setting presets from the share directory next to bin.
unzip -q xcodegen.zip
unzip -q swiftlint.zip -d swiftlint
"$RUNNER_TEMP/xcodegen/bin/xcodegen" --version
"$RUNNER_TEMP/swiftlint/swiftlint" version
printf '%s\n' "$RUNNER_TEMP/xcodegen/bin" "$RUNNER_TEMP/swiftlint" >> "$GITHUB_PATH"
