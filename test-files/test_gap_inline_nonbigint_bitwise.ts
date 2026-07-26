// Inline non-BigInt bitwise fast path (PERRY_INLINE_NONBIGINT_BITWISE).
//
// Bitwise ops (`^ | & << >> >>>`) whose operands are NOT statically numeric but
// are provably NOT BigInt must lower inline (ToInt32 <op> ToInt32) with
// byte-identical output. Operands that COULD be a BigInt must still bail to the
// BigInt-aware dynamic helper: a mixed bigint/number op throws TypeError, and
// bigint^bigint computes a BigInt. Compared byte-for-byte against Node.

// (a) typed-array reads + an `any`/erased accumulator (bcryptjs `_encipher`
//     Feistel shape). `l`/`r`/`n` init from an Int32Array element, then only
//     ever bitwise-updated — the flow analysis proves them non-BigInt, so every
//     `^`/`&`/`>>`/`>>>` on them lowers inline instead of calling a helper.
function feistel(P: Int32Array, S: Int32Array): number {
  let l: any = P[0], r: any = P[1];
  for (let i = 0; i < 6; i++) {
    let n: any = S[(l >>> 23) & 0x1ff];
    n ^= S[(l >> 7) & 0x1ff];
    n &= 0x7fffffff;
    r ^= n ^ P[i & 7];
    l ^= (n << 1) ^ S[r & 0x1ff] ^ (r >> 3);
  }
  return (l ^ r) | 0;
}
const P = new Int32Array(8);
const S = new Int32Array(512);
for (let i = 0; i < 8; i++) P[i] = ((i * 2654435761) ^ (i << 20)) | 0;
for (let i = 0; i < 512; i++) S[i] = ((i * 40503) ^ (i << 13) ^ 0x9e3779b9) | 0;
console.log("feistel:", feistel(P, S));

// (b) untyped `any[]` element reads: an `any` element COULD be a BigInt, so
//     these correctly keep the dynamic bail — results still match Node.
function untyped(a: any, b: any): void {
  console.log("untyped:", a[0] ^ b[1], a[1] & b[0], (a[0] << 3) | b[1], a[2] >>> 1, a[0] >> 1);
}
untyped([5, 9, 17], [12, 6]);

// (c) possibly-undefined out-of-bounds typed-array read (`undefined` →
//     ToInt32 = 0) — exercises the NaN-safe guarded ToInt32.
const t = new Int32Array(2);
t[0] = 123; t[1] = -456;
console.log("oob:", t[5] ^ t[0], t[5] | 0, t[5] & 0xff, (t[1] ^ t[5]) >>> 0, t[5] << 4);
// an `undefined` literal operand hits the inline path with a NaN operand.
console.log("undef:", ((undefined as any) ^ t[0]), ((undefined as any) | 5), ((undefined as any) >>> 0));

// (d) BigInt operands — the bail MUST be preserved.
console.log("bigbig:", 5n ^ 3n, 12n & 10n, 1n << 4n, 255n >> 2n, 6n | 1n, -1n & 0xffn);
function mustThrow(label: string, f: () => unknown): void {
  try {
    f();
    console.log(label, "NO_THROW");
  } catch (e) {
    console.log(label, e instanceof TypeError);
  }
}
const big: any = 7n;
const num: any = 4;
mustThrow("mix_xor:", () => big ^ num);
mustThrow("mix_and:", () => num & big);
mustThrow("mix_or:", () => big | num);
mustThrow("mix_shl:", () => big << num);

// (d') a proven-non-BigInt accumulator (`rr` — Int32Array-sourced, bitwise
//     updated) xor'd with a `bigint ^ bigint` sub-expression: the sub-expr is a
//     BigInt, so `rr ^ (na ^ nb)` is `number ^ bigint` and MUST throw. Guards
//     against the inline fast path swallowing the TypeError just because the
//     sub-expression is a (statically non-numeric) bitwise op.
const na: any = 6n, nb: any = 3n;
let rr: any = t[0];
rr ^= 5;
let nestedThrew = false;
try {
  const _x = rr ^ (na ^ nb);
  void _x;
} catch (e) {
  nestedThrew = e instanceof TypeError;
}
console.log("nested_bigint:", nestedThrew, "rr=" + (rr | 0));
