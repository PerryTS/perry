// Inherited-access lane (object/chain_store.rs): stores that must consult the prototype chain, each primed on a site first,
// then the chain changes, then the SAME site stores again. Output must match node.
const out: string[] = [];
function log(s: string) { out.push(s); }

// T1: accessor installed on the class prototype after the site learned "plain add"
class A1 { constructor() {} }
function setK1(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK1(new A1(), i);
let seen1 = -1;
Object.defineProperty(A1.prototype, "k", { set(v: number) { seen1 = v; }, get() { return 7; }, configurable: true });
{ const o: any = new A1(); setK1(o, 42); log("T1 " + seen1 + " own=" + Object.prototype.hasOwnProperty.call(o, "k") + " k=" + o.k); }

// T2: non-writable inherited data property installed after priming (sloppy: silently ignored)
class A2 { constructor() {} }
function setK2(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK2(new A2(), i);
Object.defineProperty(A2.prototype, "k", { value: 5, writable: false, configurable: true });
{ const o: any = new A2(); setK2(o, 43); log("T2 own=" + Object.prototype.hasOwnProperty.call(o, "k") + " k=" + o.k); }

// T3: strict-mode store onto a non-writable inherited property must throw
class A3 { constructor() {} }
function setK3(o: any, v: number) { "use strict"; o.k = v; }
for (let i = 0; i < 3; i++) setK3(new A3(), i);
Object.defineProperty(A3.prototype, "k", { value: 5, writable: false, configurable: true });
{ const o: any = new A3(); try { setK3(o, 44); log("T3 no-throw"); } catch (e) { log("T3 threw " + (e instanceof TypeError)); } }

// T4: setPrototypeOf on the instance to an object with a setter
class A4 { constructor() {} }
function setK4(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK4(new A4(), i);
let seen4 = -1;
{ const o: any = new A4(); Object.setPrototypeOf(o, { set k(v: number) { seen4 = v; } }); setK4(o, 45); log("T4 " + seen4 + " own=" + Object.prototype.hasOwnProperty.call(o, "k")); }

// T5: setter installed on Object.prototype after priming
class A5 { constructor() {} }
function setK5(o: any, v: number) { o.zz5 = v; }
for (let i = 0; i < 3; i++) setK5(new A5(), i);
let seen5 = -1;
Object.defineProperty(Object.prototype, "zz5", { set(v: number) { seen5 = v; }, configurable: true });
{ const o: any = new A5(); setK5(o, 46); log("T5 " + seen5 + " own=" + Object.prototype.hasOwnProperty.call(o, "zz5")); }
delete (Object.prototype as any).zz5;

// T6: frozen and non-extensible receivers
class A6 { constructor() {} }
function setK6(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK6(new A6(), i);
{ const o: any = Object.freeze(new A6()); setK6(o, 47); log("T6a own=" + Object.prototype.hasOwnProperty.call(o, "k")); }
{ const o: any = Object.preventExtensions(new A6()); setK6(o, 48); log("T6b own=" + Object.prototype.hasOwnProperty.call(o, "k")); }

// T7: a Proxy spliced into the class chain
class A7 { constructor() {} }
function setK7(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK7(new A7(), i);
let trapped7 = -1;
Object.setPrototypeOf(A7.prototype, new Proxy({}, { set(_t, _p, v) { trapped7 = v; return true; } }));
{ const o: any = new A7(); setK7(o, 49); log("T7 " + trapped7 + " own=" + Object.prototype.hasOwnProperty.call(o, "k")); }

// T8: ES5 constructor whose prototype is REPLACED after priming; old instance keeps old chain
function F8(this: any) {}
function setK8(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK8(new (F8 as any)(), i);
const old8: any = new (F8 as any)();
let seen8 = -1;
(F8 as any).prototype = { set k(v: number) { seen8 = v; } };
{ const o: any = new (F8 as any)(); setK8(o, 50); log("T8a " + seen8 + " own=" + Object.prototype.hasOwnProperty.call(o, "k")); }
{ setK8(old8, 51); log("T8b " + seen8 + " own=" + Object.prototype.hasOwnProperty.call(old8, "k") + " k=" + old8.k); }

// T9: a class setter declared in a subclass: same site, parent instances add, child instances call
class P9 { constructor() {} }
let seen9 = -1;
class C9 extends P9 { set k(v: number) { seen9 = v; } }
function setK9(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK9(new P9(), i);
{ const o: any = new C9(); setK9(o, 52); log("T9 " + seen9 + " own=" + Object.prototype.hasOwnProperty.call(o, "k")); }

// T10: delete of the prototype setter after priming a setter call: the store must now ADD
class A10 { constructor() {} }
let seen10 = -1;
Object.defineProperty(A10.prototype, "k", { set(v: number) { seen10 = v; }, configurable: true });
function setK10(o: any, v: number) { o.k = v; }
for (let i = 0; i < 3; i++) setK10(new A10(), i);
delete (A10.prototype as any).k;
{ const o: any = new A10(); setK10(o, 53); log("T10 " + seen10 + " own=" + Object.prototype.hasOwnProperty.call(o, "k") + " k=" + o.k); }

// T11: shadowing store of an inherited METHOD (Zod's this.m = this.m.bind(this)) keeps working
class A11 { m() { return 1; } constructor() { const t: any = this; t.m = t.m.bind(this); } }
{ let s = 0; for (let i = 0; i < 4; i++) { const o: any = new A11(); s += o.m() + (Object.prototype.hasOwnProperty.call(o, "m") ? 10 : 0); } log("T11 " + s); }

// T12: many layouts through one constructor site (tsc NodeObject), then a key already own
class N12 { constructor(k: number) { const t: any = this; t.pos = k; t.end = k + 1; t.kind = k; if (k & 1) t.odd = true; t.pos = k * 2; } }
{ let s = 0; for (let i = 0; i < 8; i++) { const o: any = new N12(i); s += o.pos + o.end + (o.odd ? 100 : 0) + Object.keys(o).length; } log("T12 " + s); }

// T13: T1 with a 7-byte key (the inline dyn-IC transition probe's key band), class and plain object
class A13 { constructor() {} }
function setK13(o: any, v: number) { o.kkkkkkk = v; }
for (let i = 0; i < 3; i++) setK13(new A13(), i);
let seen13 = -1;
Object.defineProperty(A13.prototype, "kkkkkkk", { set(v: number) { seen13 = v; }, configurable: true });
{ const o: any = new A13(); setK13(o, 54); log("T13a " + seen13 + " own=" + Object.prototype.hasOwnProperty.call(o, "kkkkkkk")); }
const P13: any = {};
function mk13(): any { return Object.create(P13); }
function setJ13(o: any, v: number) { o.jjjjjjj = v; }
for (let i = 0; i < 3; i++) setJ13(mk13(), i);
let seen13b = -1;
Object.defineProperty(P13, "jjjjjjj", { set(v: number) { seen13b = v; }, configurable: true });
{ const o: any = mk13(); setJ13(o, 55); log("T13b " + seen13b + " own=" + Object.prototype.hasOwnProperty.call(o, "jjjjjjj")); }
function setL13(o: any, v: number) { o.lllllll = v; }
for (let i = 0; i < 3; i++) setL13({ a: 1 }, i);
let seen13c = -1;
Object.defineProperty(Object.prototype, "lllllll", { set(v: number) { seen13c = v; }, configurable: true });
{ const o: any = { a: 1 }; setL13(o, 56); log("T13c " + seen13c + " own=" + Object.prototype.hasOwnProperty.call(o, "lllllll")); }
delete (Object.prototype as any).lllllll;

// T14: same ES5 constructor, two prototypes alive at once. The site is primed on an
// instance of the OLD prototype AFTER the replacement, then stores to an instance of
// the NEW prototype, whose setter must run (only the receiver's prototype differs).
function F14(this: any) {}
const old14a: any = new (F14 as any)();
const old14b: any = new (F14 as any)();
const old14c: any = new (F14 as any)();
let seen14 = -1;
(F14 as any).prototype = { set k(v: number) { seen14 = v; } };
function setK14(o: any, v: number) { o.k = v; }
setK14(old14a, 1); setK14(old14b, 2); setK14(old14c, 3);
{ const o: any = new (F14 as any)(); setK14(o, 57); log("T14 " + seen14 + " own=" + Object.prototype.hasOwnProperty.call(o, "k") + " old=" + old14c.k); }

console.log(out.join("\n"));
