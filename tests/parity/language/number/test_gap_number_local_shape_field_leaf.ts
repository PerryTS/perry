// #10777 / charter step 5L: `h = h + o.a` with `o = new C(...)` is a Number
// accumulator by construction when `o` is a shape-proven receiver and every
// reachable store into `a` is Number-producing. The leaf is unconditional (the
// `PERRY_L14_NBC_ORDER` knob is gone), so each case below must still match Node
// wherever the field proof has to refuse: a conditional or missing constructor
// store, a delete, string stores through `any`, an escaping sink, a computed
// key, a method, Object.assign, a getter, and a NaN field value.
class C { a: number; b: number;
  constructor(a: number, b: number, f: boolean) { if (f) this.a = a; this.b = b; } }
function condStore(n: number) { const o = new C(1, 2, false); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
class D { a: number; constructor(a?: number) { this.a = a as number; } }
function missingArg(n: number) { const o = new D(); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
class E { a: number; constructor(a: number) { this.a = a; } }
function deleted(n: number) { const o = new E(3); delete (o as any).a; let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
function strWrite(n: number) { const o = new E(3); (o as any).a = "x"; let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
function sink(x: any) { x.a = "s"; }
function escaped(n: number) { const o = new E(3); sink(o); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
function computed(n: number, k: string) { const o = new E(3); (o as any)[k] = "c"; let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
class F { a: number; constructor(a: number) { this.a = a; } set(v: any) { this.a = v; } }
function viaMethod(n: number) { const o = new F(3); o.set("m"); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
class G { a: number = 1; b: number; constructor(b: number) { this.b = b + this.a; } }
function initField(n: number) { const o = new G(2); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a + o.b; } return h; }
function nanPay(n: number) { const o = new E(0 / 0); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return [h, Object.is(h, NaN), String(h)]; }
function objAssign(n: number) { const o = new E(3); Object.assign(o, { a: "oa" }); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
function defProp(n: number) { const o = new E(3); Object.defineProperty(o, "a", { get() { return "g"; } }); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } return h; }
const results: any[] = [condStore(3), missingArg(3), deleted(3), strWrite(3), escaped(3), computed(3, "a"), viaMethod(3), initField(3), nanPay(3), objAssign(3), defProp(3)];
class P { a: number; b: number; constructor(a: number, b: number) { this.a = a; this.b = b; } }
function accumulate(n: number) { const o = new P(1.5, 2); let h = 0; for (let i = 0; i < n; i++) { h = h + o.a; } o.b = h; return o.b; }
results.push(accumulate(1000), accumulate(0));
console.log(results.map((r) => String(r)).join(" "));
