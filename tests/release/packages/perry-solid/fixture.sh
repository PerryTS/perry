#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "--__did-skip-marker" ]] && exit 1
cd "$(dirname "$0")"
source ../_fixture_lib.sh
fixture_dir="$PWD"
package_dir="$(cd ../../../../packages/perry-solid && pwd)"
mkdir -p work
cp "$package_dir/package.json" "$package_dir/package-lock.json" work/
cp -R "$package_dir/src" "$package_dir/test" work/
cd work
npm ci --ignore-scripts --no-audit --no-fund > install.log 2>&1
fixture_setup perry-solid
node --conditions=browser test/renderer.test.ts > node-out.txt
diff -u "$fixture_dir/expected.txt" node-out.txt
PERRY_DISABLE_BUILD_CACHE=1 fixture_compile_run_diff perry-solid test/renderer.test.ts "$fixture_dir/expected.txt"
if ! grep -Eq 'Found [0-9]+ module\(s\): [1-9][0-9]* native, 0 JavaScript' perry-compile.log; then
  echo 'FAIL perry-solid — expected every module to compile natively'
  exit 1
fi
cp perry-compile.log perry-renderer-compile.log
node test/jsx/oracle.cjs
node --conditions=browser test/jsx/generated.ts > jsx-node-out.txt
diff -u "$fixture_dir/expected-jsx.txt" jsx-node-out.txt
PERRY_DISABLE_BUILD_CACHE=1 fixture_compile_run_diff perry-solid-jsx test/jsx/main.tsx "$fixture_dir/expected-jsx.txt"
if ! grep -Eq 'Found [0-9]+ module\(s\): [1-9][0-9]* native, 0 JavaScript' perry-compile.log; then
  echo 'FAIL perry-solid-jsx — expected every module to compile natively'
  exit 1
fi
