#!/usr/bin/env bash
set -euo pipefail

# Regression: imported multi-level native classes must replay inherited
# constructor state all the way up the chain. This mirrors the Three.js shape
# PerspectiveCamera -> Camera -> Object3D, where Object3D assigns object-valued
# instance state and Camera redeclares some inherited fields with `declare`.

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

cat >"$TMPDIR/base_object.ts" <<'TS'
export class MatrixLike {
  label: string;

  constructor(label: string) {
    this.label = label;
  }

  decompose() {
    return "decompose:" + this.label;
  }
}

export class BaseObject {
  constructor() {
    Object.defineProperties(this, {
      hiddenMarker: { value: "hidden", configurable: true }
    });
    (this as any).plainBase = "base";
    (this as any).matrixWorld = new MatrixLike("world");
  }

  updateMatrixWorld(force?: boolean) {
    return (this as any).matrixWorld.decompose() + ":" + String(force);
  }
}
TS

cat >"$TMPDIR/camera_base.ts" <<'TS'
import { BaseObject, MatrixLike } from "./base_object.ts";

export class CameraBase extends BaseObject {
  declare matrixWorld: MatrixLike;
  declare matrixWorldInverse: MatrixLike;

  constructor() {
    super();
    (this as any).matrixWorldInverse = new MatrixLike("inverse");
    (this as any).cameraReady = true;
  }
}
TS

cat >"$TMPDIR/perspective_like.ts" <<'TS'
import { CameraBase } from "./camera_base.ts";
import { MatrixLike } from "./base_object.ts";

export class PerspectiveLike extends CameraBase {
  constructor(fov: number, aspect: number, near: number, far: number) {
    super();
    (this as any).isPerspectiveLike = true;
    (this as any).projectionMatrix = new MatrixLike(
      "projection:" + fov + ":" + aspect + ":" + near + ":" + far
    );
  }
}
TS

cat >"$TMPDIR/main.ts" <<'TS'
import { BaseObject } from "./base_object.ts";
import { CameraBase } from "./camera_base.ts";
import { PerspectiveLike } from "./perspective_like.ts";

const camera: any = new PerspectiveLike(50, 1, 0.1, 100);
console.log("isPerspective", camera.isPerspectiveLike === true);
console.log("instanceofCamera", camera instanceof CameraBase);
console.log("instanceofBase", camera instanceof BaseObject);
console.log("projection", camera.projectionMatrix.decompose());
console.log("plainBase", camera.plainBase);
console.log("matrixWorld", camera.matrixWorld.decompose());
console.log("update", camera.updateMatrixWorld(false));
console.log("inverse", camera.matrixWorldInverse.decompose());
TS

cd "$TMPDIR"
"$PERRY" compile --no-cache --no-auto-optimize main.ts --output test_bin >/dev/null
RUN_OUTPUT="$(./test_bin 2>&1)"

EXPECTED="isPerspective true
instanceofCamera true
instanceofBase true
projection decompose:projection:50:1:0.1:100
plainBase base
matrixWorld decompose:world
update decompose:world:false
inverse decompose:inverse"

if [[ "$RUN_OUTPUT" == "$EXPECTED" ]]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: imported multi-level inherited constructor state was not preserved"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
