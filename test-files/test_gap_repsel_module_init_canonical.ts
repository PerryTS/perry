// Gap test: representation-selection in a MODULE-INIT body (#7109).
//
// Correctness net for lifting the entry-context gate. Every candidate here is
// declared at module top level, which is the population that could not select
// a canonical rep before #7109, so this file is where a wrong-value regression
// from that change would show up first.
//
// Obligations exercised (each has a distinct failure signature):
//   1. canonical i32/u32 storage is the ONLY storage — a value read back in a
//      boxed context must materialize identically (sitofp / uitofp), including
//      above 2^31 for u32.
//   2. a top-level binding that a function or closure reads is a module global
//      or a boxed capture, never a canonical slot — reading it from both sides
//      must agree.
//   3. canonical Str at top level: alias demote, SSO -> heap growth, `.length`,
//      `===`, `charCodeAt`, non-ASCII bytes, and a non-string right-hand side
//      (which must ToString-coerce, not silently drop).
//   4. control flow that skips a top-level `Stmt::Let` (try/catch, switch
//      fallthrough) must read the same value Node reads.
//   5. GC: a top-level Str accumulator and an object graph must survive a
//      collection triggered in the middle of module init.
//
// Run the oracle on the Node pinned in `.node-version` at the repo root (26.5.1
// at the time of writing) — CI reads that file via `setup-node`, and Node patch
// releases change observable output, so a different local Node will not match:
//
//   node --version   # must equal "v$(tr -d 'v \n' < .node-version)"
//   node --experimental-strip-types test_gap_repsel_module_init_canonical.ts
//
// Also run the compiled binary under PERRY_CANONICAL_I32_LOCALS=0,
// PERRY_CANONICAL_STR_LOCALS=0 and PERRY_GC_FORCE_EVACUATE=1 — all four must
// agree with the oracle byte-for-byte.

// ── 1. canonical i32 / u32 at top level ───────────────────────────────────
const LIMIT = 40;
const cells: number[] = [];
for (let i = 0; i < LIMIT; i++) {
    cells[i] = (i * 7) | 0;
}
let total = 0;
for (let i = 0; i < LIMIT; i++) {
    total = total + cells[i];
}
console.log("i32:", LIMIT, cells[0], cells[LIMIT - 1], total);

// The i32 slot is signed; reading it in a boxed context must sitofp, and the
// negative wrap must be observable exactly as Node computes it.
let wrap = 2147483647 | 0;
wrap = (wrap + 1) | 0;
console.log("i32-wrap:", wrap, wrap - 1, String(wrap));

// u32: every write is a top-level `>>> 0`, so values above 2^31 must read back
// unsigned (uitofp), not as the negative signed reinterpretation.
let mix = 0x9e3779b9 >>> 0;
for (let s = 0; s < 5; s++) {
    mix = (mix ^ (mix << 13)) >>> 0;
    mix = (mix ^ (mix >>> 17)) >>> 0;
}
console.log("u32:", mix, mix.toString(16), mix > 2147483647);

// ── 2. top-level bindings that escape ─────────────────────────────────────
// `shared` is read from a function body, so it is backed by a module global and
// must NOT be canonical; `captured` is read from a closure. Both must agree
// with the module-init side.
let shared = 0;
for (let i = 0; i < LIMIT; i++) {
    shared = (shared + i) | 0;
}
function readShared(): number {
    return shared;
}
let captured = "cap";
const readCaptured = (): string => captured + "!";
captured = captured + "-more";
console.log("escape:", shared, readShared(), captured, readCaptured());

// ── 3. canonical Str at top level ─────────────────────────────────────────
let text = "";
for (let i = 0; i < 30; i++) {
    text = text + "x";
}
console.log("str:", text.length, text === "x".repeat(30), text.charCodeAt(0));

// Alias demote: `snapshot` shares the buffer, so the next `+=` must allocate
// fresh instead of mutating in place.
let grow = "ab".repeat(4);
const snapshot = grow;
grow += "Z";
console.log("str-alias:", grow, snapshot, grow.length, snapshot.length);

// Non-string right-hand side must ToString-coerce through the fast arm.
let coerced = "n";
coerced += 42;
coerced += true;
coerced += [1, 2];
console.log("str-coerce:", coerced, coerced.length);

// Non-ASCII must stay byte-exact through the append/length/compare arms.
let uni = "";
for (let i = 0; i < 4; i++) {
    uni = uni + "héllo→";
}
console.log("str-unicode:", uni.length, uni === "héllo→".repeat(4), uni.charCodeAt(1));

// ── 4. control flow that skips a top-level Let ────────────────────────────
try {
    const skipped = JSON.parse("{ not json");
    console.log("unreachable:", skipped);
} catch {
    console.log("catch-ok");
}

let switched = 0;
switch (LIMIT % 3) {
    case 0:
        switched = 100;
        break;
    case 1: {
        const inner = 7 | 0;
        switched = (switched + inner) | 0;
        break;
    }
    default:
        switched = -1;
}
console.log("switch:", switched);

// ── 5. GC pressure around top-level canonical values ──────────────────────
const graph: { id: number; label: string }[] = [];
let acc = "";
for (let i = 0; i < 200; i++) {
    graph.push({ id: i | 0, label: "n" + i });
    acc = acc + "-";
}
let live = 0;
for (let i = 0; i < graph.length; i++) {
    live = (live + graph[i].id) | 0;
}
console.log("gc:", graph.length, acc.length, live, graph[199].label, text.length);
