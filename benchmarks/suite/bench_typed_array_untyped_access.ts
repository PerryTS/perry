// Benchmark: untyped vs typed typed-array element access (issue #5525)
//
// bcryptjs's Blowfish core reaches its `S`/`P` Int32Array boxes through
// *untyped* function parameters and does ~600M `S[i]`/`P[i]` reads. Each
// untyped element read carries a per-element kind guard (the
// `PERRY_TA_KIND_CACHE` probe + view-guard + kind dispatch) that the
// statically-typed-array path skips — historically ~6x, and even after the
// inline-load fast path (#5537/#5544/#5551) still measurably above the typed
// path because the `any` receiver is shadow-stack-rooted, so LLVM cannot hoist
// the loop-invariant guard. This benchmark pins that gap: the `untyped/typed`
// ratio is the #5525 metric. Closing it to ~1.0 needs static typed-array-kind
// inference for `any` params; this guard also protects the 28s -> ~7s win
// already landed from regressing back toward the per-element thread-local
// `lookup_typed_array_kind` cost.
//
// Mirrors bcryptjs `_encipher`'s inner loop shape: P/S reached as untyped
// params, indexed with `>>>`/`|` expressions, accumulated with `+`/`^`.

function encipherUntyped(lr: any, off: number, P: any, S: any): void {
  let n: number;
  let l = lr[off];
  let r = lr[off + 1];
  l ^= P[0];
  let i = 0;
  while (i < 16) {
    n = S[l >>> 24];
    n += S[0x100 | ((l >> 16) & 0xff)];
    n ^= S[0x200 | ((l >> 8) & 0xff)];
    n += S[0x300 | (l & 0xff)];
    r ^= n ^ P[++i];
    n = S[r >>> 24];
    n += S[0x100 | ((r >> 16) & 0xff)];
    n ^= S[0x200 | ((r >> 8) & 0xff)];
    n += S[0x300 | (r & 0xff)];
    l ^= n ^ P[++i];
  }
  lr[off] = r ^ P[17];
  lr[off + 1] = l;
}

function encipherTyped(
  lr: number[],
  off: number,
  P: Int32Array,
  S: Int32Array,
): void {
  let n: number;
  let l = lr[off];
  let r = lr[off + 1];
  l ^= P[0];
  let i = 0;
  while (i < 16) {
    n = S[l >>> 24];
    n += S[0x100 | ((l >> 16) & 0xff)];
    n ^= S[0x200 | ((l >> 8) & 0xff)];
    n += S[0x300 | (l & 0xff)];
    r ^= n ^ P[++i];
    n = S[r >>> 24];
    n += S[0x100 | ((r >> 16) & 0xff)];
    n ^= S[0x200 | ((r >> 8) & 0xff)];
    n += S[0x300 | (r & 0xff)];
    l ^= n ^ P[++i];
  }
  lr[off] = r ^ P[17];
  lr[off + 1] = l;
}

const P = new Int32Array(18);
const S = new Int32Array(1024);
for (let i = 0; i < P.length; i++) P[i] = (i * 40503 + 7) | 0;
for (let i = 0; i < S.length; i++) S[i] = (i * 2654435761) | 0;

// ~4.27M calls ≈ one bcryptjs cost-12 `compareSync`'s _encipher count.
const CALLS = 4270000;

const lrU = [0x01234567, 0x89abcdef];
let t = Date.now();
for (let c = 0; c < CALLS; c++) encipherUntyped(lrU, 0, P, S);
const untyped = Date.now() - t;

const lrT = [0x01234567, 0x89abcdef];
t = Date.now();
for (let c = 0; c < CALLS; c++) encipherTyped(lrT, 0, P, S);
const typed = Date.now() - t;

// Same input → identical state proves the untyped fast path matches the typed
// path bit-for-bit (the #5525 inline path must never diverge from the slow one).
// Fail fast — a divergence is a miscompile, not a slow number, so this guard
// aborts rather than printing a sentinel.
if (lrU[0] !== lrT[0] || lrU[1] !== lrT[1]) {
  throw new Error(
    `checksum mismatch: untyped=[${lrU[0]},${lrU[1]}] typed=[${lrT[0]},${lrT[1]}]`,
  );
}

// The untyped/typed ratio is the #5525 metric: it normalizes out machine speed
// (Node ≈ 1.0 — no gap; Perry > 1 until static typed-array-kind inference
// lands). Emit it first so a single-line metric consumer (the suite runner reads
// only the first `label:number` line) tracks the *gap*, not absolute time.
const ratio = untyped / typed;
console.log("ta_untyped_typed_ratio:" + ratio);
console.log("ta_untyped_access:" + untyped);
console.log("ta_typed_access:" + typed);
console.log("checksum:" + lrU[0]);
