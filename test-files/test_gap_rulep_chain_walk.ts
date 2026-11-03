// Rule P (CJS14-DESIGN §2, DEC 253) fixture, route chain_walk.
// A compiled store through ⊤ may add a key to a builtin prototype only once
// that prototype reached ⊤ (its escape route here: a prototype-chain walk from a class instance). The add must be
// seen by every later read, compiled or not; the other builtin prototypes
// stay closed. Runtime parity with node; LIVE before rule P (main treats every ⊤ store as a write of every builtin prototype).
// Expected node output:
// 7 7 7 true
// undefined x
const g: any = globalThis;
const k = "rp" + process.argv.length;
class A { x = 1; }
let P: any = new A();
while (Object.getPrototypeOf(P) !== null) P = Object.getPrototypeOf(P);
g[k] = P;
g[k].zz = 7;
function rd(o: any): any { return o.zz; }
console.log(rd({ a: 1 }), rd(new A()), rd([]), P === Object.prototype);
console.log(rd(Object.create(null)), Object.keys(new A()).join(","));
