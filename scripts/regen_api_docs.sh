#!/usr/bin/env bash
# Regenerate Perry's API reference docs + .d.ts from the compile-time
# manifest (#465). Idempotent — committing the diff means the docs
# stayed in sync with the manifest. CI runs this and fails if the
# working tree drifts.
#
# Inputs : crates/perry-api-manifest/src/entries.rs (the source of truth)
# Outputs: docs/src/api/reference.md, docs/api/perry.d.ts

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> Building perry (release)…"
PERRY="$(cargo build --release -p perry --message-format=json-render-diagnostics | python3 -c '
import json, sys
executables = set()
for line in sys.stdin:
    message = json.loads(line)
    if (message.get("reason") == "compiler-artifact"
            and message.get("target", {}).get("name") == "perry"
            and "bin" in message.get("target", {}).get("kind", [])
            and message.get("executable")):
        executables.add(message["executable"])
if len(executables) != 1:
    sys.exit("Expected exactly one Perry executable from Cargo")
print(executables.pop())
')"

mkdir -p "$ROOT/docs/src/api" "$ROOT/docs/api"
markdown="$(mktemp "$ROOT/docs/src/api/.reference.md.XXXXXX")"
declarations="$(mktemp "$ROOT/docs/api/.perry.d.ts.XXXXXX")"
trap 'rm -f "$markdown" "$declarations"' EXIT

echo "==> Writing docs/src/api/reference.md…"
"$PERRY" --print-api-manifest=markdown > "$markdown"

echo "==> Writing docs/api/perry.d.ts…"
"$PERRY" --print-api-manifest=dts > "$declarations"
mv "$markdown" "$ROOT/docs/src/api/reference.md"
mv "$declarations" "$ROOT/docs/api/perry.d.ts"

echo "==> Done. Diff if anything changed:"
git -C "$ROOT" diff --stat -- docs/src/api/reference.md docs/api/perry.d.ts || true
