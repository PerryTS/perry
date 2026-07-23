#!/bin/bash
# Regression for the single-pass descriptor decode (#6748 follow-up): the
# fast decode must be indistinguishable from the per-field spec path. Pins
# the exact conditions under which it must FALL BACK: descriptors with
# inherited fields (custom [[Prototype]]), accessor-backed fields (literal
# getters), class instances as descriptors, and Object.prototype pollution.
# Differential: perry output must byte-match node.
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
[ ! -f "$PERRY" ] && { echo "SKIP: perry binary not found"; exit 0; }
command -v node >/dev/null 2>&1 || { echo "SKIP: node not found"; exit 0; }
TMPDIR=$(mktemp -d); trap "rm -rf $TMPDIR" EXIT
COMPILE_ENV=()
{ [ -f "$SCRIPT_DIR/../target/debug/libperry_runtime.a" ] || [ -f "$SCRIPT_DIR/../target/release/libperry_runtime.a" ]; } && COMPILE_ENV=(env PERRY_NO_AUTO_OPTIMIZE=1)
cat > "$TMPDIR/main.ts" << 'EOF'
const show = (o: any, k: string) => {
  const d = Object.getOwnPropertyDescriptor(o, k)!;
  console.log(k, d.value, typeof d.get, d.writable, d.enumerable, d.configurable);
};
// 1. plain literal descriptors (the fast-decoded shape)
const a: any = {};
Object.defineProperty(a, "p1", { value: 1 });
Object.defineProperty(a, "p2", { value: 2, writable: true, enumerable: true, configurable: true });
Object.defineProperty(a, "p3", { enumerable: true, get: () => 3 });
show(a, "p1"); show(a, "p2"); show(a, "p3"); console.log("p3-read", a.p3);
// 2. INHERITED descriptor fields (custom proto) must be honored
const dproto = { value: 42, enumerable: true };
const dchild = Object.create(dproto);
Object.defineProperty(a, "inh", dchild);
show(a, "inh");
// 3. ACCESSOR-BACKED descriptor fields must fire
let fired = 0;
const dacc = { get value() { fired++; return 7; }, enumerable: true };
Object.defineProperty(a, "acc", dacc);
show(a, "acc"); console.log("getter-fired", fired >= 1);
// 4. class INSTANCE as descriptor with prototype getter named `value`
class DescLike { get value() { return 9; } }
Object.defineProperty(a, "cls", new DescLike());
show(a, "cls");
// 5. Object.prototype pollution honored (then cleaned)
(Object.prototype as any).enumerable = true;
Object.defineProperty(a, "pol", { value: 5 });
show(a, "pol");
delete (Object.prototype as any).enumerable;
Object.defineProperty(a, "unpol", { value: 6 });
show(a, "unpol");
// 6. attrs retention + accessor->data + data->accessor transitions
const b: any = {};
Object.defineProperty(b, "t", { value: 1, writable: true, enumerable: true, configurable: true });
Object.defineProperty(b, "t", { get: () => 2, configurable: true });
show(b, "t"); console.log("t-read", b.t);
Object.defineProperty(b, "t", { value: 3 });
show(b, "t");
// 7. freeze/seal still enforced through the decode
const c: any = { x: 1 }; Object.freeze(c);
let threw = false; try { Object.defineProperty(c, "x", { value: 2 }); } catch { threw = true; }
console.log("frozen-throws", threw, c.x);
EOF
cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
P=$(./test_bin 2>&1); N=$(node main.ts 2>&1)
if [ "$P" = "$N" ]; then echo "PASS"; exit 0; fi
echo "FAIL"; echo "--- node ---"; echo "$N"; echo "--- perry ---"; echo "$P"; diff <(echo "$N") <(echo "$P") || true; exit 1
