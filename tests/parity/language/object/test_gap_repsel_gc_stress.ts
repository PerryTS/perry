// GC x representation-selection stress.
//
// Every other file in the representation corpus is a small, fast correctness
// test -- and measurably performs ZERO garbage collections, which makes every
// GC env-var arm inert against it (see docs/representation-selection-rfc.md
// 5.6 and issues #6942 / #6946). This file is the corpus member that actually
// reaches the collector: each representation-selected local is held live
// ACROSS escaping allocation churn, so a collection can land while the
// representation's value is in flight.
//
// The shape of every function here is deliberate:
//   * the representation-selected local is initialized BEFORE the churn loop,
//   * `churn()` allocates objects that escape into a module-level sink (so the
//     allocations cannot be optimized away and the arena actually grows),
//   * the local is READ AFTER the churn call in the same iteration, so a
//     collection at the allocation point must not have invalidated it.
//
// Keep the churn budget in sync with the harness: `scripts/gc_repsel_matrix.sh`
// reports how many GC cycles each arm actually drove (`PERRY_GC_TRACE=1`), and
// this file is the corpus member expected to be LIVE. If a collector change
// raises the trigger, this test silently stops collecting -- the harness
// reports it as UNVERIFIED rather than green, which is the signal to re-tune.

class Point {
    x: number;
    y: number;
    tag: string;
    constructor(x: number, y: number, tag: string) {
        this.x = x;
        this.y = y;
        this.tag = tag;
    }
    norm(): number {
        return this.x * this.x + this.y * this.y;
    }
}

// Escaping allocation churn. The sink is module-level and reassigned, so the
// allocations are genuinely live for a while and then genuinely dead -- the
// pattern that grows the arena and produces sweepable garbage.
let churnSink: unknown[] = [];
let churnAcc = 0;

function churn(i: number): void {
    churnSink.push({ i: i, s: "c" + (i & 1023), a: [i, i + 1] });
    if (churnSink.length > 4096) {
        churnAcc = (churnAcc + churnSink.length) | 0;
        churnSink = [];
    }
}

// (1) canonical unboxed i32 locals + (2) Ptr<Shape> proven object local.
// `p` is a shape-proven object local read on every iteration after the churn
// call; `acc` / `i` are canonical i32 locals with no shadow binding at all.
function shapeAndI32(n: number): number {
    const p = new Point(3, 4, "point");
    let acc = 0;
    let i = 0;
    while (i < n) {
        churn(i);
        acc = (acc + p.x * 2 + p.y) | 0;
        i = (i + 1) | 0;
    }
    return (acc + p.norm() + p.tag.length) | 0;
}

// (3) canonical string locals (Str tagged-at-rest). `s` is a string local
// re-assigned and read across the churn call; short values stay in the NaN box
// (SSO), longer ones are heap strings that a collection can sweep or move.
function strLocals(n: number): number {
    let s = "";
    let total = 0;
    for (let i = 0; i < n; i++) {
        churn(i);
        s = i & 1 ? "v" + (i & 63) : "a-heap-resident-string-value-" + (i & 63);
        total = (total + s.length) | 0;
        if (s === "v7") {
            total = (total + 1) | 0;
        }
    }
    return total | 0;
}

// (4) Ptr<NumArray> — guard-free numeric element access on a proven local.
// `arr` is allocated with a static length and only ever element-read/written
// with numeric values, which is the Phase 4a.3 eligibility shape.
function numArray(n: number): number {
    const arr = new Array(64);
    for (let k = 0; k < 64; k++) {
        arr[k] = k;
    }
    let sum = 0;
    for (let i = 0; i < n; i++) {
        churn(i);
        const k = i & 63;
        arr[k] = (arr[k] + 1) | 0;
        sum = (sum + arr[k]) | 0;
    }
    return sum | 0;
}

// (5) spec-ABI raw typed-array param (`TaPtr`). `buf` arrives as a raw
// storage pointer and is re-read after every churn call -- if the callee-side
// binding were dropped and the header moved, this is where it would show.
function taSum(buf: Int32Array, n: number): number {
    let acc = 0;
    for (let i = 0; i < n; i++) {
        churn(i);
        acc = (acc + buf[i & 255]) | 0;
    }
    return acc | 0;
}

// A second spec-ABI shape: raw param + an escaping object allocated in the
// same frame, so the frame has both a raw pointer param and a movable object.
function taMix(buf: Int32Array, n: number): number {
    const p = new Point(7, 9, "mix");
    let acc = 0;
    for (let i = 0; i < n; i++) {
        churn(i);
        acc = (acc + buf[i & 255] + p.x) | 0;
    }
    return (acc + p.y) | 0;
}

const N = 320000;

const ta = new Int32Array(256);
for (let i = 0; i < 256; i++) {
    ta[i] = (i * 3) & 255;
}

console.log("shapeAndI32", shapeAndI32(N));
console.log("strLocals", strLocals(N));
console.log("numArray", numArray(N));
console.log("taSum", taSum(ta, N));
console.log("taMix", taMix(ta, N));
console.log("churnAcc", churnAcc);
console.log("churnSink", churnSink.length);
