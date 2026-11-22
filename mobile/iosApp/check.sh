#!/bin/sh
# The iPhone checks: the structure rules, then SwiftLint.
set -eu
cd "$(dirname "$0")"
python3 -I ../tools/check_structure.py --root ..
swiftlint lint --strict
