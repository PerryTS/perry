#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "--__did-skip-marker" ]] && exit 1
cd "$(dirname "$0")"
source ../_fixture_lib.sh
fixture_dir="$PWD"
mkdir -p work
cp package.json package-lock.json tsconfig.json host.ts main.tsx oracle.mjs work/
cd work
npm ci --ignore-scripts --no-audit --no-fund > install.log 2>&1
fixture_setup opentui-solid
node oracle.mjs
node --conditions=browser main.oracle.ts > oracle-out.txt
diff -u "$fixture_dir/expected.txt" oracle-out.txt
PERRY_DISABLE_BUILD_CACHE=1 fixture_compile_run_diff opentui-solid main.tsx "$fixture_dir/expected.txt"
grep -q '5 native, 0 JavaScript' perry-compile.log
