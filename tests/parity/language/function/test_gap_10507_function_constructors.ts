// #10507: plain function constructors — `new F()` (with and without an
// object return), F.prototype replaced after instances exist, `instanceof`
// across a reassigned prototype, Symbol.hasInstance, bound functions,
// `new.target`, F called without `new`, ES5 inheritance, a shared prototype,
// closure-made constructors and many constructions surviving collections.
function A(this: any, v: number) { this.v = v; }
A.prototype.get = function (this: any) { return this.v; };
const a1: any = new (A as any)(1);
console.log("a1", a1.v, a1.get(), a1 instanceof A, Object.getPrototypeOf(a1) === A.prototype, a1.constructor === A);
// returning an object overrides this
function R(this: any) { this.x = 1; return { y: 2 }; }
const r: any = new (R as any)();
console.log("ret-obj", r.x, r.y, r instanceof R);
function RP(this: any) { this.x = 1; return 5; }
const rp: any = new (RP as any)();
console.log("ret-prim", rp.x, rp instanceof RP);
// prototype replaced after instances exist
function P(this: any) { this.k = 1; }
P.prototype.m = function () { return "old"; };
const p1: any = new (P as any)();
const oldProto = P.prototype;
P.prototype = { m() { return "new"; } };
const p2: any = new (P as any)();
console.log("replaced", p1.m(), p2.m(), p1 instanceof P, p2 instanceof P, Object.getPrototypeOf(p1) === oldProto, Object.getPrototypeOf(p2) === P.prototype);
// instanceof across a reassigned prototype, back again
P.prototype = oldProto;
console.log("restored", p1 instanceof P, p2 instanceof P);
// Symbol.hasInstance on a function
function H(this: any) {}
Object.defineProperty(H, Symbol.hasInstance, { value: (v: any) => v === 42 });
console.log("hasInstance", (42 as any) instanceof (H as any), new (H as any)() instanceof (H as any));
// bound function
function B(this: any, a: number, b: number) { this.s = a + b; }
const BB: any = (B as any).bind(null, 10);
const bb: any = new BB(5);
console.log("bound", bb.s, bb instanceof B, bb instanceof BB);
// new.target
function NT(this: any) { this.t = new.target === NT; this.u = typeof new.target; }
const nt: any = new (NT as any)();
console.log("new.target", nt.t, nt.u);
function NT2(this: any) { return typeof new.target; }
console.log("called", (NT2 as any)());
// called without new
function S(this: any, v: number): any { if (!(this instanceof S)) return new (S as any)(v); this.v = v; }
const s1: any = (S as any)(3);
const s2: any = new (S as any)(4);
console.log("self-new", s1.v, s2.v, s1 instanceof S, s2 instanceof S);
// inheritance ES5 style
function Base(this: any, n: string) { this.n = n; }
Base.prototype.hi = function (this: any) { return "hi " + this.n; };
function Child(this: any, n: string) { (Base as any).call(this, n); this.c = true; }
Child.prototype = Object.create(Base.prototype);
Child.prototype.constructor = Child;
const ch: any = new (Child as any)("z");
console.log("inherit", ch.hi(), ch instanceof Child, ch instanceof Base, ch.constructor === Child, Object.getPrototypeOf(Object.getPrototypeOf(ch)) === Base.prototype);
// shared prototype between two functions
function F1(this: any) { this.a = 1; }
function F2(this: any) { this.b = 2; }
F2.prototype = F1.prototype;
const f1: any = new (F1 as any)(), f2: any = new (F2 as any)();
console.log("shared", f1 instanceof F2, f2 instanceof F1, Object.getPrototypeOf(f2) === F1.prototype);
// fields, keys, JSON, class name print
function Pt(this: any, x: number, y: number) { this.x = x; this.y = y; }
const pts: any[] = [];
for (let i = 0; i < 50; i++) pts.push(new (Pt as any)(i, i * 2));
console.log("keys", Object.keys(pts[3]).join(","), JSON.stringify(pts[49]), pts[10].x + pts[10].y);
console.log(String(Object.getPrototypeOf(pts[0]) === Pt.prototype), pts[0].hasOwnProperty("x"), "x" in pts[0], Pt.prototype.isPrototypeOf(pts[2]));
// prototype method added after construction
(Pt.prototype as any).len = function (this: any) { return Math.sqrt(this.x * this.x + this.y * this.y); };
console.log("late", pts[3].len().toFixed(3));
// arrow is not a constructor
try { const Arr: any = () => 1; new Arr(); console.log("arrow constructed"); } catch (e: any) { console.log("arrow", e instanceof TypeError); }
// closure-made constructors (decimal-like)
function mk() { function D(this: any, v: number): any { if (!(this instanceof D)) return new (D as any)(v); this.v = v; } D.prototype.plus = function (this: any, y: number) { return new (this.constructor as any)(this.v + y); }; D.prototype.constructor = D; return D; }
const D1: any = mk(), D2: any = mk();
const d1 = new D1(1).plus(2), d2 = D2(5);
console.log("closure-ctor", d1.v, d2.v, d1 instanceof D1, d1 instanceof D2, d2 instanceof D2);
// getters on prototype, Object.create on a fn prototype
Object.defineProperty(Pt.prototype, "sum", { get(this: any) { return this.x + this.y; } });
console.log("getter", pts[4].sum, Object.create(Pt.prototype) instanceof Pt);
// setPrototypeOf on an instance
const moved: any = new (Pt as any)(1, 1);
Object.setPrototypeOf(moved, A.prototype);
console.log("setproto", moved instanceof Pt, moved instanceof A);
// many instances surviving GC
let keep: any[] = [];
for (let i = 0; i < 200000; i++) { const o: any = new (Pt as any)(i, 1); if (i % 1000 === 0) keep.push(o); }
let tot = 0; for (const o of keep) tot += o.len() > 0 && o instanceof Pt ? o.x : 0;
console.log("gc", keep.length, tot);
