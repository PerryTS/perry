// Inherited-access lane (object/chain_store.rs): stores that must consult the
// prototype chain. Each trap runs ONE store site in a loop whose trip count is
// only known at run time (so the site is neither unrolled nor cloned), primes
// it on the first iterations, changes the chain mid-loop, and records what the
// SAME site does afterwards. Output must match node.
const N = Number(process.argv[2] || 6);
const CHANGE = N - 2;
const out: string[] = [];
function log(s: string) { out.push(s); }
function own(o: any, k: string) { return Object.prototype.hasOwnProperty.call(o, k); }

// T1: accessor installed on the class prototype
class A1 { constructor() {} }
function setK1(o: any, v: number) { o.k = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.defineProperty(A1.prototype, "k", { set(v: number) { seen = v; }, get() { return 7; }, configurable: true });
    const o: any = new A1(); setK1(o, i); r.push(seen + "/" + own(o, "k"));
  }
  log("T1 " + r.join(" ")); }

// T2: non-writable inherited data property (sloppy store is ignored)
class A2 { constructor() {} }
function setK2(o: any, v: number) { o.k = v; }
{ const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.defineProperty(A2.prototype, "k", { value: 5, writable: false, configurable: true });
    const o: any = new A2(); setK2(o, i); r.push(own(o, "k") + ":" + o.k);
  }
  log("T2 " + r.join(" ")); }

// T3: strict store onto a non-writable inherited property throws
class A3 { constructor() {} }
function setK3(o: any, v: number) { "use strict"; o.k = v; }
{ const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.defineProperty(A3.prototype, "k", { value: 5, writable: false, configurable: true });
    const o: any = new A3(); try { setK3(o, i); r.push("ok:" + own(o, "k")); } catch (e) { r.push("TypeError=" + (e instanceof TypeError)); }
  }
  log("T3 " + r.join(" ")); }

// T4: setPrototypeOf on the instance to an object with a setter
class A4 { constructor() {} }
function setK4(o: any, v: number) { o.k = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N; i++) {
    const o: any = new A4();
    if (i >= CHANGE) Object.setPrototypeOf(o, { set k(v: number) { seen = v; } });
    setK4(o, i); r.push(seen + "/" + own(o, "k"));
  }
  log("T4 " + r.join(" ")); }

// T5: setter installed on Object.prototype
class A5 { constructor() {} }
function setK5(o: any, v: number) { o.zz5 = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.defineProperty(Object.prototype, "zz5", { set(v: number) { seen = v; }, configurable: true });
    const o: any = new A5(); setK5(o, i); r.push(seen + "/" + own(o, "zz5"));
  }
  delete (Object.prototype as any).zz5;
  log("T5 " + r.join(" ")); }

// T6: frozen and non-extensible receivers
class A6 { constructor() {} }
function setK6(o: any, v: number) { o.k = v; }
{ const r: string[] = [];
  for (let i = 0; i < N; i++) {
    let o: any = new A6();
    if (i === CHANGE) o = Object.freeze(o);
    if (i === CHANGE + 1) o = Object.preventExtensions(o);
    setK6(o, i); r.push(String(own(o, "k")));
  }
  log("T6 " + r.join(" ")); }

// T7: a Proxy spliced into the class chain
class A7 { constructor() {} }
function setK7(o: any, v: number) { o.k = v; }
{ let trapped = -1; const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.setPrototypeOf(A7.prototype, new Proxy({}, { set(_t, _p, v) { trapped = v; return true; } }));
    const o: any = new A7(); setK7(o, i); r.push(trapped + "/" + own(o, "k"));
  }
  log("T7 " + r.join(" ")); }

// T8: ES5 constructor whose prototype is replaced; instances of the old and new
// prototype alternate through the same site afterwards
function F8(this: any) {}
function setK8(o: any, v: number) { o.k = v; }
{ let seen = -1; const r: string[] = []; const olds: any[] = [];
  for (let i = 0; i < N; i++) olds.push(new (F8 as any)());
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) (F8 as any).prototype = { set k(v: number) { seen = v; } };
    const o: any = i >= CHANGE ? new (F8 as any)() : olds[i]; setK8(o, i); r.push(seen + "/" + own(o, "k"));
    if (i >= CHANGE) { setK8(olds[i], 100 + i); r.push("old:" + own(olds[i], "k")); }
  }
  log("T8 " + r.join(" ")); }

// T9: a subclass instance with a class setter reaches a site primed on the parent
class P9 { constructor() {} }
let seen9 = -1;
class C9 extends P9 { set k(v: number) { seen9 = v; } }
function setK9(o: any, v: number) { o.k = v; }
{ const r: string[] = [];
  for (let i = 0; i < N; i++) { const o: any = i >= CHANGE ? new C9() : new P9(); setK9(o, i); r.push(seen9 + "/" + own(o, "k")); }
  log("T9 " + r.join(" ")); }

// T10: a prototype setter DELETED mid-loop: the store must now add an own key
class A10 { constructor() {} }
let seen10 = -1;
Object.defineProperty(A10.prototype, "k", { set(v: number) { seen10 = v; }, configurable: true });
function setK10(o: any, v: number) { o.k = v; }
{ const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) delete (A10.prototype as any).k;
    const o: any = new A10(); setK10(o, i); r.push(seen10 + "/" + own(o, "k"));
  }
  log("T10 " + r.join(" ")); }

// T11: shadowing an inherited METHOD (zod's this.m = this.m.bind(this))
class A11 { m() { return 1; } constructor() { const t: any = this; t.m = t.m.bind(this); } }
{ let s = 0; for (let i = 0; i < N; i++) { const o: any = new A11(); s += o.m() + (own(o, "m") ? 10 : 0); } log("T11 " + s); }

// T12: one constructor site over many layouts, then a key already own
class N12 { constructor(k: number) { const t: any = this; t.pos = k; t.end = k + 1; t.kind = k; if (k & 1) t.odd = true; t.pos = k * 2; } }
{ let s = 0; for (let i = 0; i < N; i++) { const o: any = new N12(i); s += o.pos + o.end + (o.odd ? 100 : 0) + Object.keys(o).length; } log("T12 " + s); }

// T13: T1 with a 7-byte key (the inline dyn-IC transition key band)
class A13 { constructor() {} }
function setK13(o: any, v: number) { o.kkkkkkk = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.defineProperty(A13.prototype, "kkkkkkk", { set(v: number) { seen = v; }, configurable: true });
    const o: any = new A13(); setK13(o, i); r.push(seen + "/" + own(o, "kkkkkkk"));
  }
  log("T13 " + r.join(" ")); }

// T14: a class accessor added through the vtable-backed class chain: a subclass
// DECLARED later is not reachable, so exercise a mixin-built class instead
class B14 { constructor() {} }
function setK14(o: any, v: number) { o.k = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N; i++) {
    if (i === CHANGE) Object.defineProperty(Object.getPrototypeOf(B14.prototype), "k", { set(v: number) { seen = v; }, configurable: true });
    const o: any = new B14(); setK14(o, i); r.push(seen + "/" + own(o, "k"));
  }
  delete (Object.prototype as any).k;
  log("T14 " + r.join(" ")); }

// T15: receivers whose [[Prototype]] DIVERGED from their class (setPrototypeOf),
// alternating between two shared prototypes through one site; a setter lands
// on ONE of them only. The verdict is per recorded prototype, not per class.
class A15 { constructor() {} }
const P15: any = { tag: "p" }, Q15: any = { tag: "q" };
function setK15(o: any, v: number) { o.k = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N + 2; i++) {
    if (i === CHANGE) Object.defineProperty(Q15, "k", { set(v: number) { seen = v; }, configurable: true });
    const o: any = new A15(); Object.setPrototypeOf(o, (i & 1) ? Q15 : P15); setK15(o, i); r.push(seen + "/" + own(o, "k"));
  }
  log("T15 " + r.join(" ")); }

// T16: two receivers with the SAME key list and DIFFERENT prototypes share one
// store site (Object.create from two prototypes); a setter lands on one
// prototype only. They must not share a verdict.
const P16: any = { tag: "p" }, Q16: any = { tag: "q" };
function setK16(o: any, v: number) { o.a = 1; o.k = v; }
{ let seen = -1; const r: string[] = [];
  for (let i = 0; i < N + 2; i++) {
    if (i === CHANGE) Object.defineProperty(Q16, "k", { set(v: number) { seen = v; }, configurable: true });
    const o: any = Object.create((i & 1) ? Q16 : P16); setK16(o, i); r.push(seen + "/" + own(o, "k"));
  }
  log("T16 " + r.join(" ")); }

// T17: a class with a setter and an object created FROM its prototype share a
// store site; both must run the setter.
class C17 { _v = 0; set k(v: number) { this._v = v * 10; } }
function setK17(o: any, v: number) { o.k = v; }
{ const r: string[] = [];
  for (let i = 0; i < N; i++) {
    const o: any = (i & 1) ? new C17() : Object.create(C17.prototype); setK17(o, i);
    r.push(own(o, "k") + ":" + o._v);
  }
  log("T17 " + r.join(" ")); }

console.log(out.join("\n"));
