// Literal-length typed arrays passed to typed-array parameters (the #11810
// shape: each callee gets a specialized `$spec_ta*` clone). Once the array's
// `.buffer` has been observed, the clone may not trust the inline storage or
// the construction length: after `transfer()` it must see length 0 and
// `undefined` elements, and writes through the buffer (or a subarray) must be
// visible to the clone and back.
function sum7(a: Float64Array): number {
  let s = 0;
  for (let i = 0; i < 5; i++) { const o = i * 7; s += a[o] + a[o + 6]; }
  return s;
}
function bump(a: Float64Array): void {
  for (let i = 0; i < 35; i++) a[i] += 1;
}
const a = new Float64Array(35);
const b = new Float64Array(35);
for (let i = 0; i < 35; i++) { a[i] = i; b[i] = 2 * i; }
bump(a); bump(b);
console.log("before:", sum7(a), sum7(b));
const moved = (a.buffer as any).transfer();
console.log("detached:", sum7(a), sum7(b), a.length, a[3], new Float64Array(moved)[3]);
bump(a);
console.log("after bump:", a[0], a.length, sum7(b));

function sum8(t: Float64Array): number {
  let s = 0;
  for (let i = 0; i < 8; i++) s += t[i];
  return s;
}
function fill8(t: Float64Array, v: number): void {
  for (let i = 0; i < 8; i++) t[i] = v + i;
}
const c = new Float64Array(8);
fill8(c, 1);
console.log("fill:", sum8(c));
const w = new Float64Array(c.buffer);
w[0] = 100;
console.log("write through buffer:", sum8(c), c[0]);
fill8(c, 10);
console.log("write through clone:", w[0], w[7], sum8(c));

function isum(t: Int32Array): number {
  let s = 0;
  for (let i = 0; i < 6; i++) s += t[i];
  return s;
}
const d = new Int32Array(6);
for (let i = 0; i < 6; i++) d[i] = i;
const sub = d.subarray(1, 4);
sub[0] = 50;
console.log("subarray:", isum(d), d[1], sub.length);

// A parameter the callee passes on to code that detaches it.
let keep: ArrayBuffer | null = null;
function poke(t: Float64Array, i: number): void {
  if (i === 2) { keep = (t.buffer as any).transfer(); }
}
function run(t: Float64Array, m: number): number {
  let s = 0;
  for (let i = 0; i < m; i++) {
    poke(t, i);
    const v = t[i];
    s += v === undefined ? 1000 : v;
  }
  return s + t.length;
}
const x = new Float64Array(6);
for (let i = 0; i < 6; i++) x[i] = i + 1;
console.log("callee detaches:", run(x, 6), x.length, x[0]);
