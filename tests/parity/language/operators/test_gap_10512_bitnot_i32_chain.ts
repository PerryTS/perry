// #10512 — bitwise NOT inside a native int32 chain.
//
// Over a Number, `~x` is `x ^ -1`: ToInt32 the operand, flip every bit. Perry
// lowered `~` through a double (ToInt32 tower, xor, sitofp), and the i32 chain
// had no arm for it, so a single `~` pushed its whole enclosing `& ^ + | 0`
// expression onto the f64 path. It now stays in the chain. These cases pin the
// values that path must produce — including operands that are NOT already
// int32, where ToInt32 has to wrap the double rather than truncate it.

// ---- 1. the issue's loops: `~x` and `x ^ -1` agree ----
function sha_not(n: number): number {
  let A = 0x6a09e667 | 0, B = 0xbb67ae85 | 0, C = 0x3c6ef372 | 0, D = 0xa54ff53a | 0;
  for (let i = 0; i < n; i++) {
    const t = (((A << 25) | (A >>> 7)) ^ ((B & C) ^ (~B & D))) + i | 0;
    D = C; C = B; B = A; A = t;
  }
  return A;
}
function sha_xor(n: number): number {
  let A = 0x6a09e667 | 0, B = 0xbb67ae85 | 0, C = 0x3c6ef372 | 0, D = 0xa54ff53a | 0;
  for (let i = 0; i < n; i++) {
    const t = (((A << 25) | (A >>> 7)) ^ ((B & C) ^ ((B ^ -1) & D))) + i | 0;
    D = C; C = B; B = A; A = t;
  }
  return A;
}
function not(n: number): number {
  let x = 12345 | 0, acc = 0;
  for (let i = 0; i < n; i++) { acc = (acc + (~x & i)) | 0; x = x + 1 | 0; }
  return acc;
}
function not_xor(n: number): number {
  let x = 12345 | 0, acc = 0;
  for (let i = 0; i < n; i++) { acc = (acc + ((x ^ -1) & i)) | 0; x = x + 1 | 0; }
  return acc;
}
console.log("sha:", sha_not(100000), sha_xor(100000));
console.log("not:", not(100000), not_xor(100000));

// ---- 2. int32 boundaries ----
const edges = [0, 1, -1, 2, 255, -256, 65535, 2147483647, -2147483648, 1073741824, -1073741825];
for (const e of edges) {
  const v = e | 0;
  console.log("edge:", v, ~v, ~v & 0xff, (~v + 1) | 0, ~~v, ~v >>> 0, (~v ^ v) | 0);
}

// ---- 3. operands outside int32: ToInt32 wraps ----
let c = 1406932606;
c *= 1103515245;
c += 12345; // ~1.55e18: integer-valued, rounded, far outside int32
console.log("wide:", ~c, ~c & 0xffff, (~c ^ 7) | 0, ~~c, (~c + 3) | 0);
let h = 1406932606;
h *= h;
h *= h; // ~3.9e36: past 2^63, where a bare fptosi is poison
console.log("huge:", ~h, ~h & 0xffff, ~~h, (~h + 1) | 0);
const fr = 3.75;
const nfr = -3.75;
console.log("frac:", ~fr, ~nfr, ~fr & 7, (~nfr + 1) | 0);
console.log("special:", ~NaN, ~Infinity, ~-Infinity, ~-0, ~(2 ** 32), ~(2 ** 31), ~(-(2 ** 31) - 1), ~(2 ** 53));

// ---- 4. loop-carried integer local that leaves int32 range ----
let w = 1;
let acc = 0;
for (let i = 0; i < 45; i++) {
  w = w * 3;
  acc = (acc + (~w & 0xffff)) | 0;
}
console.log("loop-wide:", w, acc);

// ---- 5. typed-array operands and `x & ~mask` ----
const u = new Uint32Array([0xffffffff, 0x80000000, 1, 0]);
const s32 = new Int32Array([-1, -2147483648, 2147483647, 5]);
const b8 = new Uint8Array([0, 1, 127, 255]);
let s = 0;
for (let i = 0; i < 4; i++) {
  s = (s + (~u[i] & 0xff) + (~s32[i] >>> 24) + (~b8[i] & 0x1ff)) | 0;
}
console.log("typed:", s, ~u[0], ~u[1], ~s32[1], ~b8[3]);
let flags = 0b1011 | 0;
const mask = 0b0010 | 0;
console.log("clear:", flags & ~mask, (flags & ~mask) === (flags & (mask ^ -1)));
flags &= ~0b1000;
console.log("clear-assign:", flags);

// ---- 6. BigInt keeps BigInt semantics ----
const big = 5n;
console.log("bigint:", ~big, typeof ~big, ~(-1n), ~(1n << 40n));
const anyBig: any = 7n;
console.log("any-bigint:", ~anyBig, typeof ~anyBig);

// ---- 7. coercions ----
const obj = { valueOf() { return 9; } };
const arr: any = [];
console.log("coerce:", ~true, ~false, ~"12", ~"x", ~null, ~undefined, ~arr, ~obj);

// ---- 8. truthiness ----
const str = "hello";
console.log("indexOf:", ~str.indexOf("z") ? "found" : "missing", ~str.indexOf("l") ? "found" : "missing");
let bits = 0 | 0;
for (let i = 0; i < 6; i++) {
  if (~bits & 1) bits = (bits << 1) | 1;
  else bits = bits << 1;
}
console.log("cond:", bits);
