// #6794 follow-up (a): region masked-window i32 chains. The straight-line
// region versioner's ta_i32 fast copy binds locals whose every in-region write
// is strictly-i32-bounded to a region-scoped i32 shadow slot, so a
// `>>>`/`&`/`^`/`| 0` bit-mixing chain on an UNTYPED-init local (the bcryptjs
// `_encipher` shape: `l = lr[off]` is not statically i32-bounded, so `l` never
// earns a static i32 slot) stays in native i32 instead of paying a ToInt32
// tower per op. Every shape below must produce byte-identical output to Node
// whether the i32 refinement fires or the region deopts.

// Canonical Blowfish-F round shape on untyped Int32Array params, l/r init from
// a dynamic `lr[off]` read (disqualifies the static i32 slot -> exercises the
// region-scoped one).
function feistel(S: any, P: any, lr: any, off: number): number {
  let l = lr[off];
  let r = lr[off + 1];
  l = (l ^ P[0]) | 0;
  r = (r ^ ((((S[(l >>> 24) & 0xff] + S[256 + ((l >>> 16) & 0xff)]) | 0) ^ S[512 + ((l >>> 8) & 0xff)]) + S[768 + (l & 0xff)]) ^ P[1]) | 0;
  l = (l ^ ((((S[(r >>> 24) & 0xff] + S[256 + ((r >>> 16) & 0xff)]) | 0) ^ S[512 + ((r >>> 8) & 0xff)]) + S[768 + (r & 0xff)]) ^ P[2]) | 0;
  r = (r ^ ((((S[(l >>> 24) & 0xff] + S[256 + ((l >>> 16) & 0xff)]) | 0) ^ S[512 + ((l >>> 8) & 0xff)]) + S[768 + (l & 0xff)]) ^ P[3]) | 0;
  return (l ^ r) | 0;
}

// Overflow: two Int32Array elements summed feed a bitwise op — the i32 chain
// must WRAP (mod 2^32), matching `(a + b) | 0`, not saturate.
function overflow(S: any, seed: number): number {
  let x = seed | 0;
  x = (x ^ S[0]) | 0;
  x = (((S[1 + (x & 1)] + S[2 + (x & 1)]) | 0) ^ S[3 + (x & 3)]) | 0;
  x = (((S[4 + (x & 1)] + S[5 + (x & 1)]) | 0) ^ S[6 + (x & 3)]) | 0;
  x = (((S[7 + (x & 1)] + S[0]) | 0) ^ S[1]) | 0;
  x = (x ^ S[2]) | 0;
  return x | 0;
}

// Post-region: the local is read as a full Number (with a fractional add) AND
// as an unsigned int AFTER the region — the double shadow must carry the
// correct signed-i32 value out of the fast copy.
function postRead(S: any, seed: number): string {
  let x = seed | 0;
  x = (x ^ S[0]) | 0;
  x = (((S[1] + S[2]) | 0) ^ S[3]) | 0;
  x = (((S[4] + S[5]) | 0) ^ S[6]) | 0;
  x = (x ^ S[7]) | 0;
  return (x + 0.5).toFixed(1) + "|" + (x >>> 0).toString(16);
}

// Un-refine: a NON-strict (fractional Mul) write mid-region must keep the local
// off the i32 slot for the whole region, staying byte-exact.
function unrefine(S: any, seed: number): number {
  let x = seed | 0;
  x = (x ^ S[0]) | 0;
  x = (((S[1] + S[2]) | 0) ^ S[3]) | 0;
  x = x * 1.5;
  x = (((S[4] + S[5]) | 0) ^ S[6]) | 0;
  x = (x ^ S[7]) | 0;
  return (x + 100000) | 0;
}

// Plain-Array variant (untyped param that is NOT a typed array): the plain_f64
// region tier fires, where the i32-slot refinement is deliberately disabled —
// output must still match.
function feistelPlain(S: any, P: any, lr: any, off: number): number {
  let l = lr[off];
  let r = lr[off + 1];
  l = (l ^ P[0]) | 0;
  r = (r ^ ((((S[(l >>> 24) & 0xff] + S[256 + ((l >>> 16) & 0xff)]) | 0) ^ S[512 + ((l >>> 8) & 0xff)]) + S[768 + (l & 0xff)]) ^ P[1]) | 0;
  l = (l ^ ((((S[(r >>> 24) & 0xff] + S[256 + ((r >>> 16) & 0xff)]) | 0) ^ S[512 + ((r >>> 8) & 0xff)]) + S[768 + (r & 0xff)]) ^ P[2]) | 0;
  return (l ^ r) | 0;
}

const S = new Int32Array(1024);
for (let i = 0; i < 1024; i++) S[i] = ((i * 2654435761) ^ (i << 28)) | 0; // negatives + near-overflow
const P = new Int32Array(18);
for (let i = 0; i < 18; i++) P[i] = (i * 0x9e3779b1) | 0;
const lr = new Int32Array(2);

let feAcc = 0 | 0;
let ovAcc = 0 | 0;
let unAcc = 0 | 0;
for (let i = 0; i < 20000; i++) {
  lr[0] = feAcc;
  lr[1] = i;
  feAcc = (feAcc ^ feistel(S, P, lr, 0)) | 0;
  ovAcc = (ovAcc ^ overflow(S, i)) | 0;
  unAcc = (unAcc ^ unrefine(S, i)) | 0;
}
console.log("feistel=" + feAcc);
console.log("overflow=" + ovAcc);
console.log("unrefine=" + unAcc);
console.log("postRead=" + postRead(S, 0x12345678 | 0));

const Sp: number[] = new Array(1024);
for (let i = 0; i < 1024; i++) Sp[i] = ((i * 2654435761) ^ (i << 28)) | 0;
const Pp: number[] = new Array(18);
for (let i = 0; i < 18; i++) Pp[i] = (i * 0x9e3779b1) | 0;
const lrp: number[] = [0, 0];
let plainAcc = 0 | 0;
for (let i = 0; i < 20000; i++) {
  lrp[0] = plainAcc;
  lrp[1] = i;
  plainAcc = (plainAcc ^ feistelPlain(Sp, Pp, lrp, 0)) | 0;
}
console.log("feistelPlain=" + plainAcc);
