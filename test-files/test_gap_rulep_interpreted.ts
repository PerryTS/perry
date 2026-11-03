// Rule P (CJS14-DESIGN §2 route 5, DEC 150/253, owner option (b)) fixture:
// the prototype escapes only through interpreted code. `new Function` hands
// Date.prototype back as a value the facts see as ⊤ (not as Date.prototype);
// the compiled add through it is not a solver fact: once interpreted code has
// run, the runtime sets the prototype's guard word on every builtin-prototype
// write, so guarded proofs of missing members turn off. The reads after the
// add must see it. LIVE before rule P (main writes every builtin prototype on
// any ⊤ store); after rule P it is the guard word's.
// Expected node output:
// 9 9 true
// 5 5
// undefined undefined
const P: any = new Function("return Date.prototype")();
P.zz = 9;
function rd(o: any): any { return o.zz; }
console.log(rd(new Date(0)), (new Date(0) as any).zz, P === Date.prototype);
const O: any = new Function("return Object.prototype")();
O.yy = 5;
function ry(o: { a: number }): any { return (o as any).yy; }
console.log(ry({ a: 1 }), ([] as any).yy);
console.log(rd({ a: 1 }), rd([]));
