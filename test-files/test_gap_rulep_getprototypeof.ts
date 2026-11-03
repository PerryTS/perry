// Rule P (CJS14-DESIGN §2, DEC 253) fixture, route getprototypeof.
// A compiled store through ⊤ may add a key to a builtin prototype only once
// that prototype reached ⊤ (its escape route here: Object.getPrototypeOf([])). The add must be
// seen by every later read, compiled or not; the other builtin prototypes
// stay closed. Runtime parity with node; LIVE before rule P (main treats every ⊤ store as a write of every builtin prototype).
// Expected node output:
// 7 7 true
// undefined undefined undefined
const g: any = globalThis;
const k = "rp" + process.argv.length;
const P: any = Object.getPrototypeOf([]);
g[k] = P;
g[k].zz = 7;
function rd(o: any): any { return o.zz; }
console.log(rd([1]), ([] as any).zz, P === Array.prototype);
console.log(rd({ a: 1 }), rd(new Map()), ("s" as any).zz);
