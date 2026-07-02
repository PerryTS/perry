#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

mkdir -p "$TMPDIR/node_modules/dynpkg"
cat >"$TMPDIR/package.json" <<'JSON'
{
  "type": "module",
  "dependencies": {
    "dynpkg": "1.0.0"
  },
  "perry": {
    "allow": {
      "compilePackages": ["dynpkg"]
    },
    "compilePackages": ["dynpkg"]
  }
}
JSON
cat >"$TMPDIR/node_modules/dynpkg/package.json" <<'JSON'
{
  "name": "dynpkg",
  "version": "1.0.0",
  "type": "module",
  "exports": {
    "./a.js": "./a.js",
    "./b.js": "./b.js",
    "./c.js": "./c.js"
  }
}
JSON
cat >"$TMPDIR/node_modules/dynpkg/a.js" <<'JS'
export default function () { return { name: "a" }; }
JS
cat >"$TMPDIR/node_modules/dynpkg/b.js" <<'JS'
export default function () { return { name: "b" }; }
JS
cat >"$TMPDIR/node_modules/dynpkg/c.js" <<'JS'
export default function () { return { name: "c" }; }
JS
cat >"$TMPDIR/main.ts" <<'TS'
async function main() {
  const direct = await import("dynpkg/a.js");
  const registry = { b: "dynpkg/b.js" } as const;
  const viaRegistry = await import(registry.b);
  const loaders = { c: () => import("dynpkg/c.js") } as const;
  const viaLoader = await loaders.c();
  const data = { name: "not-a-real-package", port: "3000" } as const;
  let deferred = "no";
  try {
    await import(data.name);
  } catch {
    deferred = "caught";
  }
  console.log(JSON.stringify({
    direct: direct.default().name,
    registry: viaRegistry.default().name,
    loader: viaLoader.default().name,
    deferred,
  }));
}
main();
TS

BIN="$TMPDIR/main"
COMPILE_OUT="$($PERRY compile --no-cache --no-auto-optimize "$TMPDIR/main.ts" -o "$BIN" 2>&1)" || {
    echo "FAIL: perry compile errored"
    echo "$COMPILE_OUT"
    exit 1
}
OUT="$($BIN 2>&1)" || {
    echo "FAIL: compiled program errored"
    echo "$OUT"
    exit 1
}
EXPECTED='{"direct":"a","registry":"b","loader":"c","deferred":"caught"}'
if [[ "$OUT" != "$EXPECTED" ]]; then
    echo "FAIL: unexpected output"
    echo "expected: $EXPECTED"
    echo "actual:   $OUT"
    exit 1
fi

echo "PASS: dynamic import package registry"
