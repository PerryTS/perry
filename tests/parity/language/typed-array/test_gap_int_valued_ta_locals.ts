// Native-i32 residency for integer-valued locals seeded by a possibly-OOB INT
// typed-array element read (the bcryptjs `_encipher` Feistel-accumulator shape).
// `collectors/int_valued_ta_locals.rs` promotes a `let l = lr[off]` local to a
// native i32 shadow slot ONLY when every write is i32-producing and every
// observation is ToInt32-coercing; the OOB `undefined` then reads as
// `ToInt32(undefined) == 0`, indistinguishable from the f64 path. This test
// pins the SOUNDNESS boundary: an eligible accumulator whose OOB init flows
// only through bitwise ops must stay byte-exact, while an INELIGIBLE sibling
// whose read is observed as `undefined` / `String()` / `console.log` must still
// see `undefined`. Every line must match `node --experimental-strip-types`
// with PERRY_INT_VALUED_LOCALS on AND off, and under PERRY_GC_FORCE_EVACUATE=1.

const S = new Int32Array(256);
for (let i = 0; i < 256; i++) S[i] = ((i * 2654435761) ^ (i << 13) ^ 0x9e3779b9) | 0;
const P = new Int32Array(4);
for (let i = 0; i < 4; i++) P[i] = ((i * 2654435761) ^ (i << 20)) | 0;

// ---- ELIGIBLE: l/r init from an int typed-array read (index UNBOUNDED, may be
// OOB), only ever bitwise-updated, only ever read in bitwise ops / as a
// bitwise-derived index / returned as a bitwise result. Promoted to i32. ----
function mix(lr: Int32Array, off: number): number {
  let l = lr[off], r = lr[off + 1]; // OOB/negative -> undefined -> ToInt32 -> 0
  l ^= P[0];
  r ^= P[1];
  for (let round = 0; round < 8; round++) {
    l = (l ^ S[(l >>> 3) & 0xff] ^ r) | 0;
    r = (r ^ S[(r >>> 5) & 0xff] ^ l) | 0;
  }
  return (l ^ r) | 0; // bitwise result (always defined) is what is observed
}

// ---- ELIGIBLE with a typed-array element STORE observation (`lr[k] = l`): the
// stored value is ToInt32-coerced, so it is a coercing observation too. ----
function mixStore(lr: Int32Array, off: number): void {
  let l = lr[off], r = lr[off + 1];
  l ^= P[0];
  r ^= P[1];
  for (let round = 0; round < 4; round++) {
    l = (l ^ S[(l >>> 3) & 0xff] ^ r) | 0;
    r = (r ^ S[(r >>> 5) & 0xff] ^ l) | 0;
  }
  lr[off] = r; // OOB store is a no-op; in-bounds store is ToInt32(l/r)
  lr[off + 1] = l;
}

const lr = new Int32Array(2);
lr[0] = 0x1234abcd | 0;
lr[1] = 0x7654321f | 0;

console.log("mix-inbounds", mix(lr, 0));
console.log("mix-oob", mix(lr, 8)); // lr[8]/lr[9] undefined -> both accumulators start 0
console.log("mix-neg", mix(lr, -4)); // negative index -> undefined -> 0
console.log("mix-frac", mix(lr, 0.5)); // fractional index -> undefined -> 0

const st = new Int32Array(4);
st[0] = 0x0f0f0f0f | 0;
st[1] = 0x12345678 | 0;
mixStore(st, 0);
console.log("store-inbounds", st[0] | 0, st[1] | 0);
mixStore(st, 8); // OOB: reads undefined (->0), stores are no-ops
console.log("store-oob-unchanged", st[0] | 0, st[1] | 0, st[2] | 0, st[3] | 0);

// ---- INELIGIBLE: the read result is observed where `undefined` is
// distinguishable from an integer, so the analysis must NOT promote it and the
// OOB read must remain observable as `undefined`. If it were wrongly promoted,
// these would print `0`/`"0"`/`false`. ----
function eqUndef(a: Int32Array, i: number): boolean {
  const x = a[i]; // possibly OOB
  return x === undefined;
}
function strOf(a: Int32Array, i: number): string {
  const x = a[i];
  return String(x);
}
function logOf(a: Int32Array, i: number): void {
  const x = a[i];
  console.log("probe-log", x);
}
// Mixed local: read into `x`, THEN observed both ways (===undefined and String).
function mixedObs(a: Int32Array, i: number): string {
  const x = a[i];
  if (x === undefined) return "undef";
  return "num:" + String(x);
}

console.log("eq-undef", eqUndef(S, 300), eqUndef(S, -1), eqUndef(S, 3.9), eqUndef(S, 5));
console.log("str-of", strOf(S, 300), strOf(S, -1), strOf(S, 5));
logOf(S, 300);
logOf(S, 5);
console.log("mixed", mixedObs(S, 300), mixedObs(S, 5));
