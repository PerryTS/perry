#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [ ! -x "$PERRY" ]; then
  PERRY="$REPO_ROOT/target/debug/perry"
fi
if [ ! -x "$PERRY" ]; then
  echo "Perry binary not found; build target/release/perry or target/debug/perry first" >&2
  exit 1
fi

if [ -n "${NODE_BIN:-}" ]; then
  NODE="$NODE_BIN"
elif [ -x /tmp/perry-node25-bin/node ]; then
  NODE=/tmp/perry-node25-bin/node
else
  NODE="$(command -v node || true)"
fi
if [ -z "$NODE" ] || [ ! -x "$NODE" ]; then
  echo "Node binary not found" >&2
  exit 1
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat > "$TMPDIR/invalid-node.mjs" <<'NODE'
import { decode } from "node:punycode.ucs2";
console.log(decode);
NODE

if "$NODE" --no-deprecation "$TMPDIR/invalid-node.mjs" > "$TMPDIR/node.out" 2> "$TMPDIR/node.err"; then
  echo "Node unexpectedly accepted node:punycode.ucs2" >&2
  cat "$TMPDIR/node.out" "$TMPDIR/node.err" >&2
  exit 1
fi
if ! grep -Eq "ERR_UNKNOWN_BUILTIN_MODULE|No such built-in module" "$TMPDIR/node.err"; then
  echo "Node failed for an unexpected reason:" >&2
  cat "$TMPDIR/node.err" >&2
  exit 1
fi

cat > "$TMPDIR/invalid-perry.ts" <<'TS'
import { decode } from "node:punycode.ucs2";
console.log(decode);
TS

if "$PERRY" check "$TMPDIR/invalid-perry.ts" > "$TMPDIR/perry-check.out" 2>&1; then
  echo "Perry unexpectedly accepted node:punycode.ucs2" >&2
  cat "$TMPDIR/perry-check.out" >&2
  exit 1
fi
if ! grep -Fq "node:punycode.ucs2" "$TMPDIR/perry-check.out"; then
  echo "Perry failed without naming the rejected builtin:" >&2
  cat "$TMPDIR/perry-check.out" >&2
  exit 1
fi
rm -f "$TMPDIR/invalid-perry.ts"

cat > "$TMPDIR/valid-perry.ts" <<'TS'
import { ucs2 } from "node:punycode";
console.log(ucs2.decode("A").join(","));
TS
"$PERRY" check "$TMPDIR/valid-perry.ts" > "$TMPDIR/perry-valid.out" 2>&1

"$PERRY" --print-api-manifest=json > "$TMPDIR/perry-manifest.json"
"$NODE" --input-type=module - "$TMPDIR/perry-manifest.json" <<'NODE'
import fs from "node:fs";

const manifestPath = process.argv[2];
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const entries = Array.isArray(manifest) ? manifest : manifest.entries;

const ucs2Property = entries.find((entry) =>
  entry.module === "punycode" && entry.name === "ucs2" && entry.module_export === true
);
const exportedSubmoduleEntries = entries.filter((entry) =>
  entry.module === "punycode.ucs2" && entry.module_export !== false
);

if (!ucs2Property || exportedSubmoduleEntries.length) {
  console.error(JSON.stringify({ ucs2Property, exportedSubmoduleEntries }, null, 2));
  process.exit(1);
}

console.log("punycode ucs2 property exported: true");
console.log("punycode.ucs2 module exports: 0");
NODE
