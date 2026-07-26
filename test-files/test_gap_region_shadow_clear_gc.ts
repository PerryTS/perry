// #6794 follow-up (b): the masked-window region fast copies skip redundant
// per-statement shadow-slot clears (a suppressed local's slot provably stays 0).
// This must not lose a GC root: while region functions are on the stack, this
// test allocates heavily so real collections fire mid-run. Output must stay
// byte-identical to Node (and to a no-GC run) — a dropped root would corrupt a
// live pointer or crash under evacuation.

// bcryptjs `_encipher` shape: untyped Int32Array params, dynamic-init l/r,
// >130 statements so it is NOT inlined (keeps its own shadow frame).
function encipher(S: any, P: any, lr: any, off: number): number {
  let l = lr[off];
  let r = lr[off + 1];
  l = (l ^ P[0]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[1]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[2]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[3]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[4]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[5]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[6]) | 0;
  return (l ^ r) | 0;
}

const S = new Int32Array(1024);
for (let i = 0; i < 1024; i++) S[i] = ((i * 2654435761) ^ (i << 28)) | 0;
const P = new Int32Array(18);
for (let i = 0; i < 18; i++) P[i] = (i * 0x9e3779b1) | 0;
const lr = new Int32Array(2);

// Drive the region function while allocating garbage every iteration (arrays +
// strings) so the nursery fills and GC runs with `encipher` frames live.
let acc = 0 | 0;
let sink = "";
for (let i = 0; i < 40000; i++) {
  lr[0] = acc;
  lr[1] = i;
  acc = (acc ^ encipher(S, P, lr, 0)) | 0;
  const junk = [i, acc, i ^ acc, { a: i, b: acc }]; // heap allocation each iter
  if ((i & 4095) === 0) sink = "" + junk[0] + junk[2]; // occasional string build
}
console.log("acc=" + acc);
console.log("sink=" + sink);
