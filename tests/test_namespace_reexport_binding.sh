#!/usr/bin/env bash
set -euo pipefail

# Re-exporting an `import * as ns` namespace binding via a bare `export { ns }`
# must hand the importer the namespace OBJECT (with all its members), not a bare
# function symbol. Before the fix, `export { z }` of a namespace local lowered to
# `Export::Named`, so a consumer's `import { z }` resolved `z` to a function and
# every `z.<member>` was `undefined`. This is exactly how zod re-exports `z`
# (`import * as z from "./external"; export { z }`), so it broke all of zod.

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

# Mirrors zod's shape: a target module using `export *` + a direct export, a
# barrel that does `import * as z` then `export { z }` (and `export default z`).
cat >"$TMPDIR/sub.ts" <<'TS'
export function object() { return "obj"; }
TS
cat >"$TMPDIR/coerce.ts" <<'TS'
export function number() { return 42; }
TS
cat >"$TMPDIR/external.ts" <<'TS'
export * from "./sub.js";
export * as coerce from "./coerce.js";
TS
cat >"$TMPDIR/barrel.ts" <<'TS'
import * as z from "./external.js";
export * from "./external.js";
export { z };
export default z;
TS
cat >"$TMPDIR/index.ts" <<'TS'
import z4 from "./barrel.js";
export * from "./barrel.js";
export default z4;
TS
cat >"$TMPDIR/main.ts" <<'TS'
import { z } from "./barrel.js";
if (typeof z !== "object") throw new Error(`typeof z: ${typeof z}`);
if (typeof (z as any).object !== "function") throw new Error("z.object missing");
if (typeof (z as any).coerce !== "object") throw new Error("z.coerce missing");
if ((z as any).coerce.number() !== 42) throw new Error("z.coerce.number() wrong");
if ((z as any).object() !== "obj") throw new Error("z.object() wrong");
import { z as zViaExportAll } from "./index.js";
if (typeof zViaExportAll !== "object") throw new Error(`typeof zViaExportAll: ${typeof zViaExportAll}`);
if ((zViaExportAll as any).coerce.number() !== 42) throw new Error("zViaExportAll.coerce.number() wrong");
if ((zViaExportAll as any).object() !== "obj") throw new Error("zViaExportAll.object() wrong");
import * as pkg from "./barrel.js";
if (!Object.keys(pkg).includes("coerce")) throw new Error("pkg.coerce not enumerable");
if ((pkg as any).coerce.number() !== 42) throw new Error("pkg.coerce.number() wrong");
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/main.ts" 2>&1)" || {
    echo "FAIL: perry run errored"
    echo "$OUT"
    exit 1
}

if ! grep -q "^OK$" <<<"$OUT"; then
    echo "FAIL: expected OK, got:"
    echo "$OUT"
    exit 1
fi

echo "PASS: namespace re-export binding"
