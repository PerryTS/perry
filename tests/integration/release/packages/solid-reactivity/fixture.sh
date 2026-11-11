#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "--__did-skip-marker" ]] && exit 1
cd "$(dirname "$0")"
source ../_fixture_lib.sh
if [[ ! -d node_modules ]]; then
  npm ci --ignore-scripts --no-audit --no-fund
fi
fixture_setup "solid-reactivity"

# Solid's default Node export is intentionally non-reactive SSR. Use the
# client condition for the oracle, matching packageAliases in package.json.
node --conditions=browser entry.ts > node-out.txt
diff -u expected.txt node-out.txt
# A whole-build cache hit emits no module census. Keep object-cache reuse,
# but perform module collection so the native-only assertion is meaningful.
PERRY_DISABLE_BUILD_CACHE=1 fixture_compile_run_diff "solid-reactivity"
if ! grep -Eq 'Found [0-9]+ module\(s\): [1-9][0-9]* native, 0 JavaScript' perry-compile.log; then
  echo "FAIL solid-reactivity — expected every module to compile natively"
  exit 1
fi
