#!/usr/bin/env bash
set -euo pipefail

# Regression for a Three.js/DataTexture-shaped native compile edge:
# constructor field inference in a subclass must not allocate an own data slot
# for `this.image = ...` when `image` is an inherited accessor from an imported
# superclass. Type-only `declare` refinements must not shadow inherited fields.

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
if [[ "$PERRY" != /* ]]; then
  PERRY="$(cd "$(dirname "$PERRY")" && pwd)/$(basename "$PERRY")"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/base_texture.ts" <<'TS'
export const LinearFilter = 1006;

export class BaseTexture {
  public source: any;
  public version = 0;
  public magFilter: number;
  public minFilter: number;
  public generateMipmaps = true;
  public flipY = true;
  public unpackAlignment = 4;

  constructor(
    image: any = null,
    mapping: any = undefined,
    wrapS: any = undefined,
    wrapT: any = undefined,
    magFilter: number = LinearFilter,
    minFilter: number = LinearFilter,
    format: any = undefined,
    type: any = undefined,
    anisotropy: any = undefined,
    colorSpace: any = undefined,
  ) {
    this.source = { data: image, version: 0 };
    this.magFilter = magFilter;
    this.minFilter = minFilter;
  }

  get image(): any {
    return this.source.data;
  }

  set image(value: any) {
    this.source.data = value;
  }

  get readOnlyTag(): string {
    return "base-read-only";
  }

  set writeOnlyTag(value: string) {
    this.source.writeOnlyTag = value;
  }

  set needsUpdate(value: boolean) {
    if (value === true) {
      this.version++;
      this.source.version++;
    }
  }
}

export class AliasAccessorBase {
  public marker: any = "unset";

  get aliasedValue(): any {
    return this.marker;
  }

  set aliasedValue(value: any) {
    this.marker = value;
  }
}
TS

cat >"$TMPDIR/main.ts" <<'TS'
import { AliasAccessorBase, BaseTexture } from "./base_texture";

const NearestFilter = 1003;

function assert(condition: any, label: string): void {
  if (!condition) throw new Error(label);
}

class DataTextureLike extends BaseTexture {
  declare magFilter: number;
  declare minFilter: number;
  declare generateMipmaps: boolean;
  declare flipY: boolean;
  declare unpackAlignment: number;

  public isDataTexture = true;

  constructor(
    data: any = null,
    width = 1,
    height = 1,
    format: any = undefined,
    type: any = undefined,
    mapping: any = undefined,
    wrapS: any = undefined,
    wrapT: any = undefined,
    magFilter: number = NearestFilter,
    minFilter: number = NearestFilter,
    anisotropy: any = undefined,
    colorSpace: any = undefined,
  ) {
    super(null, mapping, wrapS, wrapT, magFilter, minFilter, format, type, anisotropy, colorSpace);
    this.image = { data, width, height };
    this.generateMipmaps = false;
    this.flipY = false;
    this.unpackAlignment = 1;
    this.writeOnlyTag = "ctor-setter";
  }
}

const payload = new Uint8Array(8);
const texture: any = new DataTextureLike(payload, 2, 1);

assert(texture.isDataTexture === true, "subclass field initialized");
assert(texture instanceof BaseTexture, "instanceof imported base");
assert(texture.version === 0, "initial version");
assert(texture.image !== undefined, "inherited getter returned image");
assert(texture.image.data === payload, "payload identity preserved");
assert(texture.image.width === 2, "width preserved");
assert(texture.image.height === 1, "height preserved");
assert(texture.readOnlyTag === "base-read-only", "getter-only inherited accessor read");
assert(texture.source.writeOnlyTag === "ctor-setter", "setter-only inherited accessor constructor write");
assert(texture.magFilter === NearestFilter, "magFilter subclass default");
assert(texture.minFilter === NearestFilter, "minFilter subclass default");
assert(texture.generateMipmaps === false, "subclass generateMipmaps override");
assert(texture.flipY === false, "subclass flipY override");
assert(texture.unpackAlignment === 1, "subclass unpackAlignment override");
assert(!Object.keys(texture).includes("image"), "accessor write did not create own image slot");
assert(!Object.keys(texture).includes("readOnlyTag"), "getter-only inherited accessor did not create own slot");
assert(!Object.keys(texture).includes("writeOnlyTag"), "setter-only inherited accessor did not create own slot");

class AliasAccessorChild extends AliasAccessorBase {}

const aliasProto: any = AliasAccessorChild.prototype;
aliasProto.aliasedValue = function badAliasPatch() {
  return "bad";
};

const aliasInstance: any = new AliasAccessorChild();
aliasInstance.aliasedValue = "instance-setter";
assert(aliasInstance.aliasedValue === "instance-setter", "prototype alias write preserved accessor dispatch");
assert(!Object.keys(aliasInstance).includes("aliasedValue"), "prototype alias accessor write did not create own slot");

class StaticAccessorPollution {
  static get constructorSlot(): string {
    return "static-slot";
  }

  static set constructorSlot(_value: string) {}

  constructor() {
    (this as any).constructorSlot = "instance-slot";
  }
}

const staticPollution: any = new StaticAccessorPollution();
assert(StaticAccessorPollution.constructorSlot === "static-slot", "static accessor still works");
assert(staticPollution.constructorSlot === "instance-slot", "static accessor did not suppress instance field");
assert(Object.keys(staticPollution).includes("constructorSlot"), "instance constructor assignment remained an own field");

const replacement = { data: payload, width: 4, height: 3 };
texture.image = replacement;
assert(texture.image === replacement, "inherited setter assignment round-trips");

texture.writeOnlyTag = "post-constructor";
assert(texture.source.writeOnlyTag === "post-constructor", "setter-only inherited accessor post-constructor write");

texture.needsUpdate = true;
assert(texture.version === 1, "needsUpdate increments texture version");
assert(texture.source.version === 1, "needsUpdate increments source version");

console.log("OK");
TS

cd "$TMPDIR"
"$PERRY" compile --no-cache --no-auto-optimize main.ts --output test_bin >/dev/null
RUN_OUTPUT="$(./test_bin 2>&1)"

if [[ "$RUN_OUTPUT" == "OK" ]]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: imported inherited accessor DataTexture semantics regressed"
echo "$RUN_OUTPUT"
exit 1
