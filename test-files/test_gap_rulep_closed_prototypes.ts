// Rule P (CJS14-DESIGN §2, DEC 253) fixture: no builtin prototype escapes.
// A store through ⊤ (a value loaded from globalThis under a computed key)
// writes only escaped objects: Object.prototype, Array.prototype and the
// other builtin prototypes stay closed, so their missing-member reads stay
// precise. Runtime parity with node; the precision half is the facts unit
// test `a_store_through_top_does_not_write_a_closed_builtin_prototype`
// (LIVE before rule P: the formatter import stays live; dead after).
// Expected node output:
// undefined undefined undefined
// 7 7
const g: any = globalThis;
const k = "rp" + process.argv.length;
g[k] = { own: 1 };
g[k].zz = 7;
g[k][k] = 7;
function rd(o: { a: number }): any { return (o as any).zz; }
console.log(rd({ a: 1 }), ([] as any).zz, ("s" as any)[k]);
console.log(g[k].zz, g[k][k]);
