#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../app"
npm ci
# npm skips the Electron download script.
node node_modules/electron/install.js
