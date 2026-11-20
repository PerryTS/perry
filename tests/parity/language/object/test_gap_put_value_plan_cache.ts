// Store-plan cache shared into the receiver-based [[Set]] path
// (js_put_value_set -> ordinary_set_with_receiver). Verdicts must match node.
var out: string[] = [];
// (1) inherited setter fires on receiver-set AFTER the (class,key) plan warms.
class A { _n = 0; set n(v: number){ this._n = v + 100; } get n(){ return this._n; } }
const a = new A();
for (let i = 0; i < 50000; i++) { a.n = i; }
out.push("s1=" + a.n + "|" + a._n);
// (2) plain data field on a class stays a plain store.
class B { x = 0; }
const b = new B();
for (let i = 0; i < 50000; i++) { b.x = i; }
out.push("s2=" + b.x);
// (3) per-instance setPrototypeOf override AFTER warm-up bypasses the class plan.
class C { v = 1; k = 0; }
const c1 = new C(), c2 = new C();
for (let i = 0; i < 50000; i++) { c1.k = i; }
const log: string[] = [];
Object.setPrototypeOf(c2, { set k(v: number){ log.push("ov:" + v); } });
(c2 as any).k = 7;
out.push("s3=" + log.join(",") + "|" + c1.k);
// (4) prototype accessor installed AFTER warm-up intercepts fresh instances.
class E { a = 0; }
const e1 = new E();
for (let i = 0; i < 50000; i++) { e1.w = i; }
const plog: string[] = [];
Object.defineProperty(Object.getPrototypeOf(e1), "w", { set(v: number){ plog.push("ps:" + v); }, configurable: true });
const e2 = new E();
(e2 as any).w = 5;
out.push("s4=" + plog.join(",") + "|" + (e2 as any).w);
console.log(out.join(" "));
