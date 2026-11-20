// Issue #248: `arr.push(...src)` (spread) was rejected by the LLVM
// backend with "expression ArrayPushSpread not yet supported". HIR
// already lowered the variant; only the codegen arm was missing.
// Mirrors `Expr::ArrayPush`'s realloc-aware writeback. Runtime side
// reuses `js_array_concat` which iterates the source (also handles
// Set sources via SET_REGISTRY).

console.log("=== number array spread ===");
const a: number[] = [1, 2];
const aSrc: number[] = [3, 4, 5];
a.push(...aSrc);
console.log(a.length, a.join(","));

console.log("=== string array spread ===");
const s: string[] = ["x"];
const sSrc: string[] = ["y", "z"];
s.push(...sSrc);
console.log(s.length, s.join("|"));

console.log("=== empty source ===");
const e: number[] = [10, 20];
const eSrc: number[] = [];
e.push(...eSrc);
console.log(e.length, e[0], e[1]);

console.log("=== empty destination ===");
const d: number[] = [];
const dSrc: number[] = [7, 8, 9];
d.push(...dSrc);
console.log(d.length, d.join(","));

console.log("=== spread of array literal ===");
const lit: number[] = [1];
lit.push(...[2, 3, 4]);
console.log(lit.length, lit.join(","));

console.log("=== chained push-spread ===");
const c: number[] = [];
c.push(...[1, 2]);
c.push(...[3, 4]);
c.push(...[5, 6]);
console.log(c.length, c.join(","));

console.log("=== post-spread indexOf + .length ===");
const p: number[] = [10, 20];
const pSrc: number[] = [30, 40, 50];
p.push(...pSrc);
console.log(p.indexOf(40), p.indexOf(99), p[p.length - 1]);

console.log("=== spread inside loop, growing past initial cap ===");
const big: number[] = [];
const chunk: number[] = [1, 2, 3, 4, 5];
for (let i = 0; i < 10; i++) {
  big.push(...chunk);
}
console.log(big.length, big[0], big[big.length - 1]);

console.log("=== mixed push + push-spread ===");
const m: number[] = [1];
m.push(2);
m.push(...[3, 4]);
m.push(5);
m.push(...[6, 7, 8]);
console.log(m.length, m.join(","));

console.log("=== object array spread ===");
type P = { x: number };
const objs: P[] = [{ x: 1 }];
const objsSrc: P[] = [{ x: 2 }, { x: 3 }];
objs.push(...objsSrc);
console.log(objs.length, objs[0].x, objs[1].x, objs[2].x);
