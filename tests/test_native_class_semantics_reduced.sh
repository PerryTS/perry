#!/usr/bin/env bash
set -euo pipefail

# Reduced Three-style native class semantics coverage:
#   - TypeScript `declare` class properties are type-only and must not create
#     instance fields or static slots
#   - an inherited setter-only `needsUpdate` accessor on a DataTexture-shaped
#     module boundary increments source.version, while reads remain undefined

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/source.ts" <<'TS'
export class SourceLike {
  version = 0;

  set needsUpdate(value: boolean) {
    if (value === true) {
      this.version++;
    }
  }
}
TS

cat >"$TMPDIR/texture.ts" <<'TS'
import { SourceLike } from "./source.js";

export class TextureLike {
  source = new SourceLike();

  set needsUpdate(value: boolean) {
    this.source.needsUpdate = value;
  }
}
TS

cat >"$TMPDIR/data_texture.ts" <<'TS'
import { TextureLike } from "./texture.js";

export class DataTextureLike extends TextureLike {
  declare readonly isDataTexture: true;
  declare static readonly DEFAULT_IMAGE: unknown;

  dataTag = "data-texture";
}
TS

cat >"$TMPDIR/main.ts" <<'TS'
import { DataTextureLike } from "./data_texture.js";

function hasOwn(value: any, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

const texture: any = new DataTextureLike();
if (texture.dataTag !== "data-texture") throw new Error("dataTag: " + texture.dataTag);
if (texture.source.version !== 0) throw new Error("initial version: " + texture.source.version);
if (texture.needsUpdate !== undefined) throw new Error("setter-only read should be undefined");
texture.needsUpdate = true;
if (texture.source.version !== 1) throw new Error("version after true: " + texture.source.version);
texture.needsUpdate = false;
if (texture.source.version !== 1) throw new Error("version after false: " + texture.source.version);
texture.needsUpdate = true;
if (texture.source.version !== 2) throw new Error("version after second true: " + texture.source.version);
if (hasOwn(texture, "needsUpdate")) throw new Error("setter write created own data field");
if (hasOwn(texture, "isDataTexture")) throw new Error("DataTexture declare field leaked");
if (hasOwn(DataTextureLike, "DEFAULT_IMAGE")) throw new Error("DataTexture declare static leaked");

console.log("OK");
TS

OUT="$(PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" run "$TMPDIR/main.ts" 2>&1)" || {
    echo "FAIL: perry run errored"
    echo "$OUT"
    exit 1
}
if ! grep -q "^OK$" <<<"$OUT"; then
    echo "FAIL: expected OK, got:"
    echo "$OUT"
    exit 1
fi
echo "PASS: reduced native class semantics"
