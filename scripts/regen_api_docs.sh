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
cargo build --release -p perry

PERRY="$ROOT/target/release/perry"

mkdir -p "$ROOT/docs/src/api" "$ROOT/docs/api"

echo "==> Writing docs/src/api/reference.md…"
"$PERRY" --print-api-manifest=markdown > "$ROOT/docs/src/api/reference.md"

echo "==> Writing docs/api/perry.d.ts…"
"$PERRY" --print-api-manifest=dts > "$ROOT/docs/api/perry.d.ts"

echo "==> Done. Diff if anything changed:"
git -C "$ROOT" diff --stat -- docs/src/api/reference.md docs/api/perry.d.ts || true
