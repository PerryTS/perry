// #11391: a `new F()` instance reads its own [[Prototype]] (recorded at
// construction), not F's current `.prototype`, after `F.prototype` is reassigned.
function F(this: any) {}
(F as any).prototype = { a: 1 };
const o: any = new (F as any)();
(F as any).prototype = { a: 2, b: 3 };
console.log(o.a, o.b, Object.getPrototypeOf(o) === (F as any).prototype);
o.own = 1;
console.log(o.a, o.b, o.own);
const k = "b";
console.log(o[k], "b" in o, "a" in o);
const later: any = new (F as any)();
console.log(later.a, later.b, Object.getPrototypeOf(later) === (F as any).prototype);

// Default prototype: `constructor` and methods added after reassignment.
function G(this: any) {}
(G as any).prototype.m = function () { return "old m"; };
const g: any = new (G as any)();
(G as any).prototype = { z: 9, m() { return "new m"; } };
(G as any).prototype.q = 5;
console.log(g.z, g.q, g.constructor === G, g.m());
g.x = 1;
console.log(g.z, g.q, g.constructor === G, g.m(), typeof g.m);

// Methods on the old prototype keep working through the recorded link.
function H(this: any, v: number) { this.v = v; }
(H as any).prototype = { get double() { return this.v * 2; }, show() { return "v=" + this.v; } };
const h: any = new (H as any)(4);
(H as any).prototype = { show() { return "wrong"; } };
console.log(h.double, h.show(), new (H as any)(1).show());

// A chain of `F.prototype = new G()` still resolves through every hop.
function A(this: any) {}
(A as any).prototype.fromA = "A";
function B(this: any) {}
(B as any).prototype = new (A as any)();
(B as any).prototype.fromB = "B";
const b: any = new (B as any)();
console.log(b.fromA, b.fromB, b.missing);
(B as any).prototype = {};
console.log(b.fromA, b.fromB, b.missing);

// Unchanged prototype: nothing changes.
function C(this: any) { this.c = 1; }
(C as any).prototype.greet = function () { return "hi " + this.c; };
const c: any = new (C as any)();
console.log(c.greet(), c.nope);
const keys: string[] = [];
for (const key in o) keys.push(key);
console.log(keys.join(","));
