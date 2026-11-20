// #10511: bitwise operators over operands the compiler cannot prove are
// Numbers — destructured / `number`-annotated locals seeded from property
// reads, untyped property reads, closure-captured lets — at every magnitude
// ToInt32 distinguishes, plus the non-Number operands the slow arm must keep.

class St {
  A: number = 0x6a09e667 | 0;
  B: number = 0xbb67ae85 | 0;
  C: number = 0x3c6ef372 | 0;
  D: number = 0xa54ff53a | 0;
}

function rotr(w: number, s: number): number {
  return (w << (32 - s)) | (w >>> s);
}

// noble SHA2_32B.process: `let { A, B, C, D } = this`.
function destructure(st: St, n: number): number {
  let { A, B, C, D } = st;
  for (let i = 0; i < n; i++) {
    const t = (rotr(A, 7) ^ ((B & C) ^ (~B & D))) + i | 0;
    D = C; C = B; B = A; A = t;
  }
  st.A = A; st.B = B; st.C = C; st.D = D;
  return A;
}

function annotated(st: St, n: number): number {
  let A: number = st.A, B: number = st.B, C: number = st.C, D: number = st.D;
  for (let i = 0; i < n; i++) {
    const t = (rotr(A, 7) ^ ((B & C) ^ ((B ^ -1) & D))) + i | 0;
    D = C; C = B; B = A; A = t;
  }
  return A;
}

function prop(o: any, n: number): number {
  let h = o.seed;
  for (let i = 0; i < n; i++) {
    h ^= o.k;
    h = (h << 13) | (h >>> 19);
    h = (h * 5 + 0xe6546b64) & 0xffffffff;
  }
  return h;
}

// jsbn am1: `v & 0x3ffffff` with |v| near 2^48.
function am1(o: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = o.xs[i & 63] * o.x + o.c;
    s = (s + (v & 0x3ffffff) + (v >>> 26)) % 1000000007;
  }
  return s;
}

console.log("destructure", destructure(new St(), 1000));
console.log("annotated", annotated(new St(), 1000));
const o: any = { seed: 0x1234567, k: 0x5bd1e995 | 0, x: 4194301, c: 7, xs: [] as any[] };
for (let i = 0; i < 64; i++) o.xs.push((i * 2654435761) % 67108864);
console.log("prop", prop(o, 1000));
console.log("am1", am1(o, 1000));

// nanoid: `buffer[i] & mask` with a closure-captured `let mask`.
function makeMasker(alphabetSize: number) {
  let mask = (2 << (31 - Math.clz32((alphabetSize - 1) | 1))) - 1;
  return (bytes: number[]) => bytes.map((b) => b & mask);
}
console.log("nanoid", makeMasker(64)([0, 63, 64, 200, 255, 1023]).join(","));

// Every operator over every magnitude ToInt32 splits on, read from an object
// so no operand is statically a Number.
const two63 = 2 ** 63;
const values: any = {
  list: [
    0, -0, 0.5, -0.5, 1.5, -1.5, 7, -7,
    2 ** 31 - 1, 2 ** 31, -(2 ** 31), -(2 ** 31) - 1,
    2 ** 32 - 1, 2 ** 32, 2 ** 32 + 5, 4294967295.5, -4294967297,
    2 ** 48 * 3 + 7, -(2 ** 48 * 3 + 7), 2 ** 53 - 1, 2 ** 53 + 2,
    1e18, -1e18, two63 - 1024, -(two63 - 1024), two63, -two63,
    two63 + 2048, -(two63 + 2048), 1e20, -1e20, (2 ** 52 + 1) * 2 ** 31,
    2 ** 84, 1.7976931348623157e308, -1.7976931348623157e308,
    Number.MIN_VALUE, NaN, Infinity, -Infinity,
  ],
};
const ops: [string, (a: any, b: any) => any][] = [
  ["&", (a, b) => a & b],
  ["|", (a, b) => a | b],
  ["^", (a, b) => a ^ b],
  ["<<", (a, b) => a << b],
  [">>", (a, b) => a >> b],
  [">>>", (a, b) => a >>> b],
];
const rhs: any = { list: [0, -1, 1, 5, 31, 32, 33, 0x3ffffff, 2 ** 32 + 3, -(2 ** 32) - 1, 1e20, NaN] };
for (const [name, f] of ops) {
  const rows: string[] = [];
  for (const a of values.list) {
    const row: any[] = [];
    for (const b of rhs.list) row.push(f(a, b));
    rows.push(row.join(" "));
  }
  console.log(name, rows.join(" | "));
}
console.log("~", values.list.map((v: any) => ~v).join(" "));
console.log("|0", values.list.map((v: any) => v | 0).join(" "));
console.log(">>>0", values.list.map((v: any) => v >>> 0).join(" "));

// Compound assignment on a property-seeded local.
const box: any = { h: 1e20 };
let h = box.h;
h &= 0xffff; console.log("&=", h);
h = box.h; h |= 0; console.log("|=", h);
h = box.h; h ^= -1; console.log("^=", h);
h = box.h; h <<= 3; console.log("<<=", h);
h = box.h; h >>= 3; console.log(">>=", h);
h = box.h; h >>>= 3; console.log(">>>=", h);

// Non-Number operands still take the ToNumeric path.
const odd: any = {
  list: [undefined, null, true, false, "12", "0x10", " 7 ", "abc", "", [], [5], [1, 2],
    { valueOf() { return 2 ** 40 + 9; } }, new Number(2 ** 33 + 1), new Boolean(true)],
};
for (const v of odd.list) console.log("odd", v & 0xff, v | 0, v ^ 1, v << 2, v >> 1, v >>> 0, ~v);

// BigInt operands keep BigInt semantics; mixing still throws.
const bigs: any = { a: 0x1234_5678_9abc_def0n, b: 0xffn, n: 3 };
console.log("bigint", bigs.a & bigs.b, bigs.a | bigs.b, bigs.a ^ bigs.b, bigs.a << 4n, bigs.a >> 4n, ~bigs.a);
for (const [name, f] of ops) {
  try {
    console.log("mixed", name, f(bigs.a, bigs.n));
  } catch (e) {
    console.log("mixed", name, (e as Error).constructor.name);
  }
}
try {
  console.log(bigs.a >>> bigs.b);
} catch (e) {
  console.log(">>> bigint", (e as Error).constructor.name);
}

// A valueOf that runs once per conversion, in operand order.
const seen: string[] = [];
const left: any = { valueOf() { seen.push("L"); return 2 ** 35 + 3; } };
const right: any = { valueOf() { seen.push("R"); return 1e20; } };
console.log("order", left & right, left ^ right, seen.join(""));
