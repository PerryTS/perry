// Inline-hot-small (PERRY_INLINE_HOT_SMALL, #6850 follow-up).
//
// Perry biases LLVM toward inlining a *small* function that has a *hot*
// (in-loop) call site with few total call sites, by stamping `inlinehint` and
// raising `-inlinehint-threshold`. This must not change observable behavior:
// the result of the inlined callee, its GC shadow-frame correctness across the
// inlined boundary, and the array/pointer ops it feeds must all be byte-for-byte
// identical to Node.
//
// `mix` below is a ~10-statement bit-mixer that reads an Int32Array parameter
// and calls Math.imul (the exact shape that stays out-of-line without the flag
// and gets inlined into its loop with it). It has ONE loop call site, so it is
// hinted; its result is loop-carried and feeds Int32Array writes.
//
// Run byte-for-byte vs `node --experimental-strip-types`, and (mechanism
// permitting) under PERRY_GC_FORCE_EVACUATE=1 — the inlined boundary must not
// drop a GC root.

function mix(S: Int32Array, x: number): number {
  let a = x | 0;
  a = (a ^ S[a & 1023]) | 0;
  a = Math.imul(a, 0x9e3779b1);
  a = (a ^ (a >>> 15)) | 0;
  a = (a ^ S[(a >>> 7) & 1023]) | 0;
  a = Math.imul(a, 0x85ebca6b);
  a = (a ^ (a >>> 13)) | 0;
  a = (a ^ S[(a >>> 3) & 1023]) | 0;
  a = Math.imul(a, 0xc2b2ae35);
  a = (a ^ (a >>> 16)) | 0;
  return a | 0;
}

// A second small hot callee whose result indexes into an array (pointer op).
function idx(n: number): number {
  let h = n | 0;
  h = (h ^ (h >>> 7)) | 0;
  h = Math.imul(h, 0x2545f491);
  h = (h ^ (h >>> 11)) | 0;
  h = (h + 0x7f4a7c15) | 0;
  h = (h ^ (h << 3)) | 0;
  h = Math.imul(h, 0x27d4eb2f);
  h = (h ^ (h >>> 15)) | 0;
  h = (h >>> 0) % 64;
  return h | 0;
}

const S = new Int32Array(1024);
for (let i = 0; i < 1024; i++) S[i] = ((i * 2654435761) ^ (i << 28)) | 0;

// Loop-carried call into `mix`, results written into a typed array (array ops
// fed by the inlined callee's result).
const OUT = new Int32Array(64);
let acc = 0 | 0;
for (let i = 0; i < 20000; i++) {
  acc = (acc ^ mix(S, acc ^ i)) | 0;
  // pointer/array op fed by a second inlined hot callee
  const slot = idx(acc);
  OUT[slot] = (OUT[slot] + (acc & 0xffff)) | 0;
}

console.log("acc=" + acc);

// Deterministic checksum over the array the inlined results populated.
let chk = 0 | 0;
for (let i = 0; i < 64; i++) chk = (Math.imul(chk, 31) + (OUT[i] | 0)) | 0;
console.log("chk=" + chk);

// A few spot values so a miscompile of the inlined boundary shows as a diff.
console.log("OUT[0]=" + OUT[0]);
console.log("OUT[17]=" + OUT[17]);
console.log("OUT[63]=" + OUT[63]);
console.log("mix(S,1)=" + mix(S, 1));
console.log("idx(123456)=" + idx(123456));
