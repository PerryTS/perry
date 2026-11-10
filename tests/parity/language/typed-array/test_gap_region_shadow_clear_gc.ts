// #6794 follow-up (b): the masked-window region fast copies skip redundant
// per-statement shadow-slot clears (a suppressed local's slot provably stays 0).
// This must not lose a GC root. A region-versioned function is alloc-free and
// call-free by construction (that is what makes its body region-matchable), so a
// GC can never fire *inside* its fast copy — the safety of the skip is static
// (suppression blocks every write, so the slot still holds 0). What a runtime
// test CAN guard is the broader interaction: repeatedly entering/leaving region
// functions while the program allocates enough to drive real collections must
// stay byte-identical to Node and survive forced evacuation. The gap harness
// compares this deterministic output against Node.

// Full 16-round Feistel, bcryptjs `_encipher` shape: untyped Int32Array params,
// dynamic-init l/r, and enough statements that it is never inlined (so it keeps
// its own shadow frame and the region versioner runs on the whole round chain).
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
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[7]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[8]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[9]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[10]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[11]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[12]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[13]) | 0;
  l = (l ^ ((((S[(r>>>24)&0xff] + S[256+((r>>>16)&0xff)])|0) ^ S[512+((r>>>8)&0xff)]) + S[768+(r&0xff)]) ^ P[14]) | 0;
  r = (r ^ ((((S[(l>>>24)&0xff] + S[256+((l>>>16)&0xff)])|0) ^ S[512+((l>>>8)&0xff)]) + S[768+(l&0xff)]) ^ P[15]) | 0;
  l = (l ^ P[16]) | 0;
  r = (r ^ P[17]) | 0;
  return (l ^ r) | 0;
}

const S = new Int32Array(1024);
for (let i = 0; i < 1024; i++) S[i] = ((i * 2654435761) ^ (i << 28)) | 0;
const P = new Int32Array(18);
for (let i = 0; i < 18; i++) P[i] = (i * 0x9e3779b1) | 0;
const lr = new Int32Array(2);

// Drive the region function repeatedly while allocating garbage every iteration
// (arrays + objects + occasional strings) so the nursery fills and real
// collections run across many enter/leave cycles of `encipher`.
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
