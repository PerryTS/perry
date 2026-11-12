// Codegen's integer range rules are those of doubles (`expr/range_facts.rs`,
// `PERRY_INT_RANGE_EXACT`), an i32 shift masks its count
// (`PERRY_SHIFT_COUNT_MASK`), and the literal 2^63 is no i64.
//
// Each section is a shape whose range proof was false: the proven native
// read took a truncated or out-of-bounds key, or the shift was undefined.

// ---- 1. a quotient is an integer key only where the division is exact ----
function quotientKeys(): string {
  const b = new Uint8Array(4);
  b[0] = 11; b[1] = 22; b[2] = 33; b[3] = 44;
  const out: string[] = [];
  for (let i = 0; i <= 8; i++) out.push(String(b[i / 4])); // 0.25 is no element
  for (let i = 0; i <= 12; i += 4) out.push(String(b[i / 4]));
  for (let i = 0; i <= 15; i++) out.push(String(b[(i / 4) | 0])); // control: truncated
  for (let i = 0; i <= 15; i++) out.push(String(b[i >> 2])); // control
  return out.join(",");
}
console.log("quotient:", quotientKeys());
function quotientStores(): string {
  const t = new Int32Array(8);
  for (let i = 0; i < 8; i++) t[i] = i + 1;
  let s = 0;
  for (let i = 0; i <= 14; i++) {
    t[i / 2] = 100 + i; // a store at 0.5 writes no element
    s += t[i / 2] | 0;
  }
  return s + ":" + Array.from(t).join(",");
}
console.log("quotient stores:", quotientStores());

// ---- 2. past 2^53 the i64 a rule computes is not the double the program holds ----
function rounded(): string {
  const b = new Uint8Array(201);
  for (let i = 0; i < 201; i++) b[i] = (i * 7) & 0xff;
  let s1 = 0, s2 = 0, s3 = 0;
  for (let i = 0; i <= 200; i++) s1 += b[(i + 1152921504606846976) - 1152921504606846976] | 0; // 0 or 256
  for (let i = 0; i <= 200; i++) s2 += b[i * 9007199254740993 - i * 9007199254740992] | 0;
  for (let i = 0; i <= 200; i++) s3 += b[(i + 4503599627370496) - 4503599627370496] | 0; // control: exact
  return s1 + "," + s2 + "," + s3;
}
console.log("rounded:", rounded());

// ---- 3. a constant's bitwise operators wrap to int32 ----
function masks(i: number): string {
  const b = new Uint8Array(4);
  b[0] = 1; b[1] = 2; b[2] = 3; b[3] = 4;
  const M = 6442450944; // 1.5 * 2^32: ToInt32 is -2^31, `>> 31` is -1
  let s1 = 0, s2 = 0, s3 = 0, s4 = 0;
  for (let k = 0; k < 40; k++) {
    s1 += b[(i + k) & (M >> 31)] | 0;
    s2 += b[(i + k) & (6442450944 >> 31)] | 0;
    s3 += b[(i + k) & (4294967299 & 4294967295)] | 0; // control: 3
    s4 += b[(i + k) >>> (4294967296 + 30)] | 0; // count 30
  }
  return s1 + "," + s2 + "," + s3 + "," + s4;
}
console.log("masks:", masks(0), masks(2), masks(-3));

// ---- 4. a shift count is masked to five bits, variable or not ----
function shifts(k: number): string {
  let a = 1;
  a = 1 << k;
  let c = -8;
  c = c >> k;
  let d = -8;
  d = d >>> k;
  let e = 5;
  e <<= k;
  const x = k | 0;
  return [a, c, d, e, (3 << x) | 0, (-3 >> x) | 0, (7 >>> x) >>> 0, ((x << x) ^ (x >> x)) | 0].join(",");
}
for (const k of [0, 1, 31, 32, 33, 63, 64, -1, -31, -32, 4294967295, 4294967297, 1.5, -0.5]) {
  console.log("shift:", k, shifts(k));
}
let m1 = -1;
m1 = 1 << m1;
let m2 = 33;
m2 = 3 << m2;
let m3 = 40;
m3 >>= m3;
console.log("module shifts:", m1, m2, m3);

// ---- 5. the literal 2^63 ----
function ident(x: number): number {
  return x;
}
const big = ident(9223372036854775808);
const nbig = ident(-9223372036854775808);
console.log("2^63:", big, ~big, big | 0, big >>> 0, big & 0xffff, nbig, ~nbig, nbig | 0, nbig >>> 0);
console.log("2^63 literal:", 9223372036854775808 | 0, ~9223372036854775808, 9223372036854775808 >>> 0, 9223372036854775807 | 0, -9223372036854775808 | 0);
