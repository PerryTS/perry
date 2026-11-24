// A constructor's rest parameter receives the arguments from its position on as one array:
// on `new`, through an inherited constructor, from an explicit `super(...)`, through a
// class value, for a class declared in a function and on `new` in an async function.
class P { n: number; a: number[]; constructor(first: number, ...rest: number[]) { this.n = rest.length; this.a = rest; } }
class OnlyRest { a: number[]; constructor(...rest: number[]) { this.a = rest; } }
class D extends P {}
class E extends P { constructor(x: number) { super(x, x + 1, x + 2); } }
console.log(new P(1, 2, 3, 4).n, new P(1, 2, 3, 4).a.join(","), new P(1).n, new OnlyRest().a.length, new OnlyRest(5, 6).a.join(","));
console.log(new D(1, 2, 3).a.join(","), new E(7).a.join(","));
const Q: any = P;
console.log(new Q(1, 2, 3, 4).a.join(","));
function local(v: number) { class L { r: number[]; constructor(...r: number[]) { this.r = r; } } return new L(v, v); }
console.log(local(9).r.join(","));
async function inAsync() { return new OnlyRest(1, 8, 9).a.join(",") + " " + new P(1, 8, 9).a.join(","); }
inAsync().then((v) => console.log("async " + v));
