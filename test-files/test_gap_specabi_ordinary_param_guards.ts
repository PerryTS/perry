// #8079: ordinary parameter annotations are specialization candidates, never
// proofs. Direct calls may enter a proof-bearing clone only after validating
// the live argument; annotation lies, escaped/indirect calls, and accessors
// must retain the public boxed semantics.

function arrayTotal(values: number[]): any {
  let total: any = values[0];
  for (let i = 1; i < values.length; i++) {
    total = total + values[i];
  }
  return total;
}

console.log("array-good", arrayTotal([1, 2, 3, 4]));
console.log("array-wrong-elements", arrayTotal(["x", 2, 3] as any));
console.log("array-wrong-container", arrayTotal({ 0: "o", 1: 7, length: 2 } as any));

const escapedArrayTotal = arrayTotal;
console.log("array-indirect", escapedArrayTotal(["i", 5] as any));

function choose(flag: boolean, values: number[]): number {
  let selected = values[1];
  for (let i = 0; i < 1; i++) {
    if (flag) selected = values[0];
  }
  return selected;
}

console.log("scalar-good", choose(true, [11, 22]));
console.log("scalar-wrong", choose(7 as any, [11, 22]));

function addFirst(value: number, values: number[]): any {
  let result: any = value;
  for (let i = 0; i < 1; i++) result = result + values[0];
  return result;
}

// With no raw-representation call-site fact for the first argument, its live
// string value must fail the ordinary `number` descriptor and use the boxed
// body (`"n" + 2`, not a numeric coercion).
console.log("number-wrong", addFirst("n" as any, [2]));

interface Payload {
  label: string;
  count: number;
}

function render(payload: Payload): any {
  let result: any = payload.label;
  for (let i = 0; i < 1; i++) {
    result = result + ":" + payload.count;
  }
  return result;
}

console.log("object-good", render({ label: "items", count: 3 }));
console.log("object-wrong", render({ label: 9, count: "many" } as any));

let getterHits = 0;
const accessorPayload: any = { count: 4 };
Object.defineProperty(accessorPayload, "label", {
  enumerable: true,
  get() {
    getterHits++;
    return "getter";
  },
});
console.log("object-accessor", render(accessorPayload), getterHits);

function mutatePayload(payload: Payload): any {
  (payload as any).count = "changed";
  return payload.count + 1;
}

// A parameter whose reachable value is mutated is deliberately ineligible:
// an entry guard cannot prove facts that the body itself later invalidates.
console.log("object-mutated", mutatePayload({ label: "x", count: 1 }));

type Tree =
  | { kind: "leaf"; value: number }
  | { kind: "branch"; left: Tree; right: Tree };

function treeTotal(tree: Tree): any {
  if (tree.kind === "leaf") return tree.value;
  return treeTotal(tree.left) + treeTotal(tree.right);
}

const tree: Tree = {
  kind: "branch",
  left: { kind: "leaf", value: 4 },
  right: {
    kind: "branch",
    left: { kind: "leaf", value: 5 },
    right: { kind: "leaf", value: 6 },
  },
};
console.log("union-recursive", treeTotal(tree));
console.log("union-wrong", treeTotal({ kind: "leaf", value: "bad" } as any));
console.log(
  "union-nested-wrong",
  treeTotal({
    kind: "branch",
    left: { kind: "leaf", value: "bad" },
    right: { kind: "leaf", value: 1 },
  } as any),
);

class NumericBox {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}

class StringBox {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
}

function bumpBox(box: NumericBox): any {
  let result: any = box.value;
  for (let i = 0; i < 1; i++) result = result + 1;
  return result;
}

console.log("class-good", bumpBox(new NumericBox(8)));
console.log("class-wrong", bumpBox(new StringBox("s") as any));

// Keep the guarded parameter live across enough allocations for the moving-GC
// matrix. Both the clone and a failed-guard fallback execute this body.
//
// The count is load-bearing, not arbitrary. A two-field literal is ~72 bytes,
// so the original 50_000 allocated ~3.6 MB against a 64 MB initial nursery
// threshold and triggered ZERO collections: under
// `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` the run reported
// `copying_minors=0` with no `[gc-fromspace-protect]` line, so the fixture
// would have passed unchanged while the clone held a stale pointer across an
// evacuation. 1_200_000 is ~86 MB, which clears the threshold, and dropping
// all but every 4096th object keeps a small live set churning so survivors
// evacuate out of retired blocks instead of everything promoting together.
//
// Verified: `copying_minors=20` with 20 `[gc-fromspace-protect]
// mode=ProtectPages` lines (bytes_protected=18874368 on the first retirement),
// output byte-exact against node. With the from-space pages mprotect'd, a
// guarded clone holding a stale parameter now faults at the offending
// instruction instead of passing silently.
function surviveMovingGc(payload: Payload): string {
  const before = payload.label;
  const kept: any[] = [];
  let batch: any[] = [];
  for (let i = 0; i < 1200000; i++) {
    batch.push({ i, text: "j" + (i & 1023) });
    if (batch.length >= 4096) {
      kept.push(batch[0]);
      batch = [];
    }
  }
  return before + ":" + payload.label + ":" + kept.length;
}

console.log("moving-clone", surviveMovingGc({ label: "live", count: 1 }));
console.log("moving-fallback", surviveMovingGc({ label: 9, count: "lie" } as any));

// #8094: the entry guard validates the argument ONCE, at entry. A descriptor
// proof describes a heap object, so it survives only as long as no unknown
// code can run. These three cases each broke a `b.v + 1` into an unchecked
// `fadd` on a NaN-box, which propagates the payload rather than producing NaN,
// so the wrong value passed through arithmetic unchanged and printed a
// plausible wrong answer.
//
// None of them needs a cast: `any` is assignable to `number`, so tsc accepts
// all of this.
interface AliasBox {
  v: number;
}

const aliasPoison: any = "lie";

// (a) mutation through a callee we hand the reference to.
function aliasAssign(b: AliasBox): void {
  b.v = aliasPoison;
}

function aliasThroughArgument(b: AliasBox): string {
  const before = b.v + 1;
  aliasAssign(b);
  return "before=" + before + " after=" + (b.v + 1) + " typeof=" + typeof b.v;
}

console.log("alias-argument", aliasThroughArgument({ v: 41 }));

// (b) the same hazard one level deeper, through an array element.
interface AliasRow {
  n: number;
}

function aliasTamper(rows: AliasRow[]): void {
  rows[0].n = aliasPoison;
}

function aliasThroughElement(rows: AliasRow[]): string {
  const a = rows[0].n + 1;
  aliasTamper(rows);
  return "a=" + a + " b=" + (rows[0].n + 1) + " typeof=" + typeof rows[0].n;
}

console.log("alias-element", aliasThroughElement([{ n: 10 }]));

// (c) the parameter is NEVER passed anywhere. The callee reaches it through a
// global the caller stashed it in first. This is why the fix keys on "did
// unknown code run", not on "did the reference escape": an escape analysis
// over our own argument lists answers "no escape" here and still miscompiles.
let aliasStash: any = null;

function aliasPoisonStash(): void {
  aliasStash.v = aliasPoison;
}

function aliasThroughGlobal(b: AliasBox): string {
  const before = b.v + 1;
  aliasPoisonStash();
  return "before=" + before + " after=" + (b.v + 1) + " typeof=" + typeof b.v;
}

const aliasStashed: AliasBox = { v: 41 };
aliasStash = aliasStashed;
console.log("alias-global", aliasThroughGlobal(aliasStashed));

// #8094 follow-on: `surviveMovingGc` above takes an INTERFACE parameter and
// calls `push`, so under the aliasing rule it is no longer guard-eligible and
// its clone is gone. Verified with `--trace llvm`: before the rule both
// `surviveMovingGc$spec_b` and a primitive-parameter sibling were emitted;
// after it only the primitive sibling is. That silently turned the moving-GC
// arm above into a test of the GENERIC path — a gate whose subject stopped
// running.
//
// This row restores it. `tag: string` and `rounds: number` are primitives, so
// they stay guard-eligible under the rule (a callee has no route to the
// caller's copy of a string or a number), while the body still allocates
// hard enough to force copying minors. Assert with:
//   PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_DIAG=1
// and check for a non-zero `copying_minors` plus `[gc-fromspace-protect]`
// lines; a run with zero copying minors protects nothing.
function survivePrimitiveGuardedGc(tag: string, rounds: number): string {
  const kept: any[] = [];
  let batch: any[] = [];
  for (let i = 0; i < rounds; i++) {
    batch.push({ i, text: "k" + (i & 1023) });
    if (batch.length >= 4096) {
      kept.push(batch[0]);
      batch = [];
    }
  }
  return tag + ":" + tag.length + ":" + kept.length;
}

console.log("moving-primitive", survivePrimitiveGuardedGc("live", 1200000));
console.log("moving-primitive-lie", survivePrimitiveGuardedGc(7 as any, 1200000));
