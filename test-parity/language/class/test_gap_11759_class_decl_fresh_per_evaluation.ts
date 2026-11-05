// #11759: a class DECLARATION evaluated more than once is a fresh class per
// evaluation, as in node. Perry keeps the shared (static) class for the
// FIRST evaluation and gives every later evaluation its own class object
// (option (c′)); each line below compares the two against node.

function f() { class K {} return K; }
console.log("fn-identity", f() === f());
const a: any = f(), b: any = f();
a.prototype.x = 1;
console.log("proto-isolation", new b().x, new a().x, a.prototype === b.prototype);
console.log("instanceof-across", new a() instanceof b, new b() instanceof a, new a() instanceof a, new b() instanceof b);
console.log("getproto", Object.getPrototypeOf(new b()) === b.prototype, Object.getPrototypeOf(new a()) === a.prototype);
console.log("ctor", new b().constructor === b, new a().constructor === a, new b().constructor === a);
console.log("name", a.name, b.name, typeof b, a.length, b.length);
console.log("string", String(a), String(b));

// Statics: each evaluation owns its own, including through the class name
// and `this` inside static methods.
function h() {
  class T { static count = 0; static make() { T.count++; return new T(); } }
  return T;
}
const h1: any = h(), h2: any = h();
h1.make(); h1.make(); h2.make();
console.log("static-count", h1.count, h2.count);
console.log("make-instanceof", h2.make() instanceof h2, h2.make() instanceof h1, h1.make() instanceof h1);
function hs() { class U { static tag = "u"; static who() { return this.tag; } } return U; }
const u1: any = hs(), u2: any = hs();
u1.tag = "changed";
console.log("static-this", u1.who(), u2.who());
function del() { class D { static z = 1; static m() { return 2; } } return D; }
const d1: any = del(), d2: any = del();
delete d2.z;
delete d1.m;
console.log("delete", d1.z, d2.z, typeof d1.m, typeof d2.m);
console.log("own-names", Object.getOwnPropertyNames(d1).sort().join(), Object.getOwnPropertyNames(d2).sort().join());

// Captured values and the class's own name inside its members.
function mk(tag: string) {
  class S { static t = tag; static n = 0; static inc() { return ++S.n; } who() { return S.t; } }
  S.prototype.extra = function () { return "x" + S.t; };
  return S;
}
const s1: any = mk("a"), s2: any = mk("b"), s3: any = mk("c");
s1.inc(); s1.inc(); s2.inc();
console.log("captures", s1.t, s2.t, s3.t, s1.n, s2.n, s3.n);
console.log("members", new s1().who(), new s2().who(), new s3().who(), new s1().extra(), new s3().extra());
console.log("proto-patch-isolated", s1.prototype.extra === s2.prototype.extra);
function selfref() {
  class R { static made = 0; clone() { R.made++; return new R(); } }
  return R;
}
const r1: any = selfref(), r2: any = selfref();
const c1 = new r1().clone(), c2 = new r2().clone();
console.log("selfref", c1 instanceof r1, c1 instanceof r2, c2 instanceof r2, c2 instanceof r1, r1.made, r2.made);

// Instance fields and accessors are per evaluation too.
function fld() { class P { x = 1; y = 2; get s() { return this.x + this.y; } } return P; }
const p1 = new (fld() as any)(), p2 = new (fld() as any)();
console.log("fields", p1.s, p2.s, Object.keys(p1).join(), Object.keys(p2).join());

// A definition that runs once (an IIFE) has one class, as in node.
const once: any = (function () { class O { static tag = "o"; } return O; })();
console.log("iife", once.tag, new once() instanceof once);

// Many evaluations: every one distinct and self-consistent.
function many() { class M { m() { return 1; } } return M; }
let ok = true;
const firstM = many();
for (let i = 0; i < 50; i++) {
  const M: any = many();
  if (M === firstM || !(new M() instanceof M) || new M() instanceof firstM || new M().m() !== 1) ok = false;
}
console.log("many", ok);
