// repsel #7480 / #5093: the element-shape versioned loop clone.
//
// `for (let j = 0; j < n; j++) sum += keep[j].v` is cloned behind a preheader
// guard on the per-array homogeneous element-shape invariant (#7496). Every
// case below is a way that guard, or the clone's residual per-element check,
// must decline — a stale specialized body reading a revoked array is a
// MISCOMPILE, not a slow path, which is why each hazard gets a case here and
// not just a codegen unit test.

class Node {
  v: number;
  w: number;
  constructor(v: number, w: number) {
    this.v = v;
    this.w = w;
  }
}

class Other {
  v: string;
  constructor(v: string) {
    this.v = v;
  }
}

function sumField(rows: Node[], n: number): number {
  let sum = 0;
  for (let j = 0; j < n; j++) {
    sum += rows[j].v;
  }
  return sum;
}

// ---------------------------------------------------------------------------
// 1. The hot shape. Homogeneous array, every element the same class.
// ---------------------------------------------------------------------------
const keep: Node[] = [];
for (let i = 0; i < 64; i++) {
  keep.push(new Node(i, i * 2));
}
console.log("hot:", sumField(keep, keep.length));

// Re-entering the loop must re-verify: the O(1) confirm path is what runs
// from the second visit on.
console.log("hot-again:", sumField(keep, keep.length));

// ---------------------------------------------------------------------------
// 2. Empty array — the invariant declines a vacuous proof, so the clone must
//    not be entered and the loop must simply run zero times.
// ---------------------------------------------------------------------------
const empty: Node[] = [];
console.log("empty:", sumField(empty, empty.length));

// ---------------------------------------------------------------------------
// 3. Shape mismatch — the array is homogeneous, but at the WRONG class.
// ---------------------------------------------------------------------------
const others: Other[] = [new Other("a"), new Other("b")];
let mixedOut = "";
for (let j = 0; j < others.length; j++) {
  mixedOut += others[j].v;
}
console.log("wrong-class:", mixedOut);

// A genuinely heterogeneous array: no proof exists at all.
const hetero: Node[] = [new Node(1, 1), new Node(2, 2)];
(hetero as unknown as unknown[])[1] = { v: 40, w: 0 };
let heteroOut = "";
for (let j = 0; j < hetero.length; j++) {
  heteroOut += String(hetero[j].v) + ",";
}
console.log("hetero:", heteroOut);

// ---------------------------------------------------------------------------
// 4. Holes / undefined elements. A `TAG_HOLE` store funnels through the same
//    hook that maintains the invariant, so the proof is revoked and the reads
//    must observe JavaScript's `undefined`.
// ---------------------------------------------------------------------------
const holed: Node[] = [new Node(1, 1), new Node(2, 2), new Node(3, 3)];
delete (holed as unknown as unknown[])[1];
let holeOut = "";
for (let j = 0; j < holed.length; j++) {
  const row = holed[j] as Node | undefined;
  holeOut += row === undefined ? "_" : String(row.v);
}
console.log("holes:", holeOut);

const sparse: Node[] = new Array(3) as Node[];
sparse[0] = new Node(7, 7);
let sparseOut = "";
for (let j = 0; j < sparse.length; j++) {
  const row = sparse[j] as Node | undefined;
  sparseOut += row === undefined ? "_" : String(row.v);
}
console.log("sparse:", sparseOut);

// ---------------------------------------------------------------------------
// 5. MID-LOOP REVOCATION. The store happens inside the loop that reads, so a
//    guard hoisted to the preheader would be stale from the second iteration
//    on. The matcher declines any body containing a store; this asserts the
//    observable result is unchanged either way.
// ---------------------------------------------------------------------------
const revoked: Node[] = [];
for (let i = 0; i < 8; i++) {
  revoked.push(new Node(i, i));
}
let revokedOut = "";
for (let j = 0; j < revoked.length; j++) {
  // Per-element rather than a running sum: a single NaN would swallow every
  // later read, and a miscompile that read the poisoned slot as a raw double
  // would then be indistinguishable from the correct answer.
  revokedOut += String(revoked[j].v) + ",";
  if (j === 2) {
    // Replaces a shaped element with a plain number: the invariant is
    // revoked from here on, and the remaining reads must NOT take a
    // specialized path.
    (revoked as unknown as unknown[])[5] = 100;
  }
}
console.log("mid-loop-store:", revokedOut);

// Revocation through a CALL made by the body — the same hazard one level of
// indirection away.
const viaCall: Node[] = [];
for (let i = 0; i < 8; i++) {
  viaCall.push(new Node(i, i));
}
function poison(rows: Node[], at: number): number {
  (rows as unknown as unknown[])[at] = 500;
  return 0;
}
let viaCallOut = "";
for (let j = 0; j < viaCall.length; j++) {
  viaCallOut += String(viaCall[j].v + (j === 1 ? poison(viaCall, 6) : 0)) + ",";
}
console.log("revoke-via-call:", viaCallOut);

// Revocation BETWEEN two entries of the same loop: the first sweep is
// specialized, the mutation retires the proof, the second sweep must not be.
const between: Node[] = [];
for (let i = 0; i < 16; i++) {
  between.push(new Node(i, i));
}
console.log("between-1:", sumField(between, between.length));
(between as unknown as unknown[])[4] = 1000;
let betweenOut = "";
for (let j = 0; j < between.length; j++) {
  betweenOut += String(between[j].v) + ",";
}
console.log("between-2:", betweenOut);

// ---------------------------------------------------------------------------
// 6. SUBCLASS RECEIVER (#7573/#7603). An `Array` subclass instance is a plain
//    ObjectHeader whose fields overlay ArrayHeader's, so the clone must be
//    reachable only behind a genuine GC_TYPE_ARRAY brand.
// ---------------------------------------------------------------------------
class NodeList extends Array<Node> {}

const sub: Node[] = new NodeList() as Node[];
sub.push(new Node(3, 3));
sub.push(new Node(4, 4));
sub.push(new Node(5, 5));
console.log("subclass:", sumField(sub, sub.length), sub.length);

// The same receiver read through a plainly-typed parameter, which is the
// binding form #7603 found in the wild.
function subclassSum(rows: Node[]): number {
  let sum = 0;
  for (let j = 0; j < rows.length; j++) {
    sum += rows[j].v;
  }
  return sum;
}
console.log("subclass-param:", subclassSum(sub));

// ---------------------------------------------------------------------------
// 7. Residual per-element hazards the ARRAY-level invariant does not cover:
//    the object's own keys array, its descriptor flag, and its typed-layout
//    intact bit. Each of these leaves the array homogeneous at the same class
//    id, so only the clone's per-element check can catch them.
// ---------------------------------------------------------------------------
const downgraded: Node[] = [];
for (let i = 0; i < 6; i++) {
  downgraded.push(new Node(i, i));
}
// A non-number into a `number`-typed field downgrades that object's raw-f64
// layout while class_id and the array's homogeneity are unchanged.
(downgraded[3] as unknown as Record<string, unknown>).v = "nine";
let downOut = "";
for (let j = 0; j < downgraded.length; j++) {
  downOut += String(downgraded[j].v) + ",";
}
console.log("layout-downgrade:", downOut);

const deleted: Node[] = [];
for (let i = 0; i < 6; i++) {
  deleted.push(new Node(i, i));
}
// `delete` compacts the packed slots and installs a fresh keys array while
// PRESERVING class_id — a class-id match alone does not prove the layout.
delete (deleted[2] as unknown as Record<string, unknown>).w;
let delOut = "";
for (let j = 0; j < deleted.length; j++) {
  delOut += String(deleted[j].v) + ",";
}
console.log("deleted-field:", delOut);

const accessor: Node[] = [];
for (let i = 0; i < 6; i++) {
  accessor.push(new Node(i, i));
}
Object.defineProperty(accessor[4], "v", {
  get() {
    return 99;
  },
  configurable: true,
});
let accOut = "";
for (let j = 0; j < accessor.length; j++) {
  accOut += String(accessor[j].v) + ",";
}
console.log("own-accessor:", accOut);

const frozen: Node[] = [];
for (let i = 0; i < 6; i++) {
  frozen.push(new Node(i, i));
}
Object.freeze(frozen[1]);
console.log("frozen-element:", sumField(frozen, frozen.length));

// ---------------------------------------------------------------------------
// 8. Prototype surgery on the element class retires every outstanding proof.
// ---------------------------------------------------------------------------
const protoTouched: Node[] = [];
for (let i = 0; i < 8; i++) {
  protoTouched.push(new Node(i, i));
}
console.log("proto-before:", sumField(protoTouched, protoTouched.length));
Object.defineProperty(Node.prototype, "extra", {
  get() {
    return 1;
  },
  configurable: true,
});
console.log("proto-after:", sumField(protoTouched, protoTouched.length));

// ---------------------------------------------------------------------------
// 9. Length changes: the record pins the length it was verified against, so
//    every length-changing mutation invalidates the proof on the next query.
// ---------------------------------------------------------------------------
const grow: Node[] = [new Node(1, 1), new Node(2, 2)];
console.log("grow-1:", sumField(grow, grow.length));
grow.push(new Node(3, 3));
console.log("grow-2:", sumField(grow, grow.length));
grow.pop();
console.log("grow-3:", sumField(grow, grow.length));
grow.length = 1;
console.log("grow-4:", sumField(grow, grow.length), grow.length);

// A bound larger than the array: the preheader requires the verified prefix to
// cover the whole index range, so the out-of-range reads stay generic.
const shortArr: Node[] = [new Node(5, 5), new Node(6, 6)];
let overOut = "";
for (let j = 0; j < 4; j++) {
  const row = shortArr[j] as Node | undefined;
  overOut += row === undefined ? "_" : String(row.v);
}
console.log("bound-past-length:", overOut);

// ---------------------------------------------------------------------------
// 10. GROWTH FORWARDING (#7480). Every case above builds its array in the same
//     scope that reads it, so the binding always held the LIVE head and the
//     preheader's raw-pointer derivation happened to be right. It is not right
//     in general: `js_array_grow` moves the array and leaves a forwarding stub
//     whose first payload word (length‖capacity) is overwritten with the new
//     head. The runtime resolves the chain, so the guard call still answers
//     about the live array; a preheader that derived `length` and the elements
//     base from the stub instead read the low half of a heap pointer as
//     `length` (so the bound check PASSED) and addressed the pre-growth
//     buffer — correct answers up to the initial capacity of 16, and a bus
//     error at 17.
//
//     Both shapes below cross that boundary. Keep the counts above 16.
// ---------------------------------------------------------------------------
function buildRows(n: number): Node[] {
  const rows: Node[] = [];
  for (let i = 0; i < n; i++) {
    rows.push(new Node(i, i * 2));
  }
  return rows;
}

const returned = buildRows(40);
console.log("grown-callee-built:", sumField(returned, returned.length));
// Re-entering must still be right: the binding is repaired in place, so the
// second visit takes the O(1) confirm path on an already-live head.
console.log("grown-callee-built-again:", sumField(returned, returned.length));

// A prefix of the same array — the index where a stale base first runs off the
// pre-growth block.
let prefix = 0;
for (let j = 0; j < 17; j++) {
  prefix += returned[j].v;
}
console.log("grown-prefix-17:", prefix);

// The canonical stale-head shape: the CALLER allocates, the CALLEE grows. The
// callee's write-backs update its own parameter slot, so the caller's binding
// keeps the pre-growth head.
function fill(rows: Node[], n: number): void {
  for (let i = 0; i < n; i++) {
    rows.push(new Node(i, i * 3));
  }
}

const callerOwned: Node[] = [];
fill(callerOwned, 40);
console.log("grown-callee-filled:", sumField(callerOwned, callerOwned.length));
