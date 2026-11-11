// Math.exp is Node's bit for bit, not the platform libm's (which differs in
// the last bit for about 5% of inputs). three.js bakes exp-derived Gaussian
// weights into generated shader source, so a 1-ulp difference changes output.
const xs: number[] = [];
let s = 0x2545f491;
function rnd(): number {
  s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0;
  return s / 4294967296;
}
for (let i = 0; i < 200000; i++) {
  const r = rnd();
  const m = i % 4;
  xs.push(m === 0 ? (r * 2 - 1) * 2 : m === 1 ? (r * 2 - 1) * 750 : m === 2 ? (r * 2 - 1) * 1e-6 : -708 - r * 38);
}

// FNV-1a over the result bits.
const f = new Float64Array(1);
const w = new Uint32Array(f.buffer);
function hashAll(exp: (x: number) => number): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < xs.length; i++) {
    f[0] = exp(xs[i]);
    h = Math.imul(h ^ w[0], 0x01000193) >>> 0;
    h = Math.imul(h ^ w[1], 0x01000193) >>> 0;
  }
  return h.toString(16);
}
const direct = hashAll((x) => Math.exp(x));
const asValue = hashAll(Math.exp);
// The hash differs between arm64 and x86-64 Node (FMA contraction in V8's
// build); the parity oracle is the Node on the same machine.
console.log("hash:", direct, "direct == value:", direct === asValue);

// Spot values (the same on arm64 and x86-64 Node).
const sigma = 22 / 3;
console.log(Math.exp(-0.5 * 16 / (sigma * sigma)));
console.log(Math.exp(1), Math.exp(-1), Math.exp(709.78), Math.exp(-745.13), Math.exp(-0));
console.log(Math.exp(Infinity), Math.exp(-Infinity), Math.exp(NaN), Math.exp(710), Math.exp(-746));
