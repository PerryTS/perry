function leaf(a: number, b: number): number { return 1 / (a + b + 1); }
function root(a: number): number { return Math.sqrt(a); }
function matrix(a: Float64Array, out: Float64Array) {
  for (let i = 0; i < 4; i++) {
    let total = -0;
    for (let j = 0; j < 4; j++) total += leaf(i, j) * a[j];
    out[i] = total;
  }
}
const a = new Float64Array(4), out = new Float64Array(4);
a[0] = 2; a[1] = -3; a[2] = 7; a[3] = 11;
matrix(a, out); console.log("nested", out[0], out[1], out[3]);
matrix(a, a); console.log("alias", a[0], a[1], a[3]);
function positiveStart(a: Float64Array, n: number) {
  let s = 0;
  for (let i = 1; i < n; i++) s += root(a[i]);
  return s;
}
console.log("start", positiveStart(out, 4), positiveStart(out, 0));
function signed(a: Float64Array) {
  for (let i = 0; i < 4; i++) {
    let s = -0;
    for (let j = 0; j < 2; j++) s = s * 1;
    a[i] = s;
  }
}
signed(out); console.log("sign", Object.is(out[0], -0), Object.is(out[3], -0));
function exposed(a: Float64Array) {
  const v = new Float64Array(a.buffer);
  for (let i = 0; i < 4; i++) a[i] += v[i];
}
exposed(a); console.log("exposed", a[0], a[3]);
const fake = {0: 2, 1: 4, 2: 8, 3: 16} as unknown as Float64Array;
matrix(fake, out); console.log("fake", out[0], out[3]);
const meta = new Uint8Array(4); meta[0] = 4;
Object.defineProperty(meta, "length", {get() { console.log("length-effect"); return 2; }});
function scan(a: Uint8Array) { let n = 0; for (let i = 0; i < a.length; i++) n += a[i]; return n; }
console.log("metadata", scan(meta));
function made() { const x = new Float64Array(4); for (let i = 0; i < 4; i++) x[i] = leaf(i, 1); return x; }
const fresh = made(); console.log("fresh", fresh[0], fresh[3]);
for (let i = 0; i < 80; i++) { const junk = new Float64Array(4096); matrix(fresh, out); if (i === 79) console.log("moved", out[0], junk.length); }
function boundedCoerce(a: Int32Array, seed: number) {
  for (let i = 0; i < 6; i++) { seed = (seed * 1103515245 + 12345) | 0; a[i] = seed; }
}
function hugeCoerce(a: Int32Array, seed: number) {
  for (let i = 0; i < 6; i++) { seed = (seed * 1e100 + 12345) | 0; a[i] = seed; }
}
const boundedOut = new Int32Array(6);
for (const seed of [-2147483648, 2147483647, 123456789, -0, 0.5, NaN, Infinity, 1e100]) {
  boundedCoerce(boundedOut, seed); console.log("bounded", boundedOut[0], boundedOut[1], boundedOut[5]);
  hugeCoerce(boundedOut, seed); console.log("huge", boundedOut[0], boundedOut[5]);
}
