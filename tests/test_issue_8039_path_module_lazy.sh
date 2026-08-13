#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
FIXTURE="$REPO_ROOT/test-files/issue_8039_path_modules"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: build Perry first or set PERRY_BIN"
    exit 0
fi

(
    cd "$FIXTURE"
    "$PERRY" compile --no-auto-optimize entry.js -o "$WORK/path-module-lazy"
)

OUTPUT="$(cd "$FIXTURE" && "$WORK/path-module-lazy" 2>&1)"
EXPECTED="PASS: issue 8039 cold/warm path modules"
if [[ "$OUTPUT" != *"$EXPECTED"* ]]; then
    echo "FAIL: path-module lazy graph did not pass"
    echo "$OUTPUT"
    exit 1
fi

echo "$EXPECTED"
