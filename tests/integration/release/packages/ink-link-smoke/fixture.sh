#!/usr/bin/env bash
# Tier-3 fixture: ink-link-smoke.
#
# Issue #678 is a linker regression: native code referenced perry_fn/perry_closure
# symbols for V8-fallback Ink modules. This fixture intentionally stops after
# compile/link and symbol inspection. Executing the binary currently reaches a
# broader React/Object runtime interop gap (`hasOwnProperty is not a function`),
# which is not the #678 contract.
# The locked yoga-layout package bootstraps its default export from a WASM helper.
# This fixture does not execute layout code, so we patch that default export to an
# inert object after npm install to keep the link-only guard focused on Ink's
# package graph and native symbol surface.

set -uo pipefail
cd "$(dirname "$0")"
. "../_fixture_lib.sh"

NAME="ink-link-smoke"
fixture_setup "$NAME" || exit 1

node --input-type=commonjs <<'JS'
const fs = require('fs');

const patches = [
  {
    file: 'node_modules/yoga-layout/src/index.ts',
    importBlock:
      "// @ts-ignore untyped from Emscripten\nimport loadYoga from '../binaries/yoga-wasm-base64-esm.js';\nimport wrapAssembly from './wrapAssembly.ts';\n",
  },
  {
    file: 'node_modules/yoga-layout/dist/src/index.js',
    importBlock:
      "// @ts-ignore untyped from Emscripten\nimport loadYoga from '../binaries/yoga-wasm-base64-esm.js';\nimport wrapAssembly from \"./wrapAssembly.js\";\n",
  },
];

for (const patch of patches) {
  let text = fs.readFileSync(patch.file, 'utf8');
  if (text.includes(patch.importBlock)) {
    text = text.replace(patch.importBlock, '');
  }
  text = text.replace('const Yoga = wrapAssembly(await loadYoga());', 'const Yoga = {};');
  if (!text.includes('const Yoga = {};')) {
    throw new Error(`failed to patch yoga-layout link stub in ${patch.file}`);
  }
  fs.writeFileSync(patch.file, text);
}
JS

echo "  [perry compile/link] entry.tsx"
if ! "$PERRY_BIN" compile entry.tsx -o ./out > perry-compile.log 2>&1; then
    echo "FAIL $NAME - perry compile/link errored"
    sed 's/^/    /' perry-compile.log | tail -60
    exit 1
fi

if command -v nm >/dev/null 2>&1; then
    unresolved="$(nm -u ./out 2>/dev/null | grep -E '_?perry_(fn|closure)_node_modules_ink_build_(render|measure_text)_js' || true)"
    if [[ -n "$unresolved" ]]; then
        echo "FAIL $NAME - unresolved #678 Ink symbols remain"
        printf '%s\n' "$unresolved" | sed 's/^/    /'
        exit 1
    fi
else
    echo "  [warn] nm not found; compile/link success is the symbol check"
fi

echo "PASS $NAME"
