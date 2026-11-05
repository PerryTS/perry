// DPA-F1 #2: a function-constructor family symbol store must prove the
// receiver carries exactly the constructor layout AND that the slot is a
// plain writable data property; otherwise ordinary [[Set]] (setters run,
// writable:false throws in strict code, permuted/dictionary go generic).
const s = Symbol("s");
const t = Symbol("t");
function F(this: any) { this[s] = 1; }
function G(this: any) { this[s] = 1; this[t] = 2; }
// a normal birth keeps working
const a = new (F as any)();
console.log("birth", a[s]);
// accessor with setter: [[Set]] runs it
const o: any = {};
Object.defineProperty(o, s, { get() { return "getter"; }, set(v) { console.log("setter ran", v); }, configurable: true });
F.call(o);
console.log("accessor after", o[s]);
// writable:false in strict code throws
const w: any = {};
Object.defineProperty(w, s, { value: 5, writable: false, enumerable: true, configurable: true });
try { F.call(w); console.log("no throw", w[s]); } catch (e) { console.log("threw", e instanceof TypeError, w[s]); }
// permuted receiver: t first then s
const p: any = {}; p[t] = 7; p[s] = 8;
G.call(p);
console.log("permuted", p[s], p[t], Object.getOwnPropertySymbols(p).map(String).join());
// dictionary receiver
const d: any = {}; for (let i = 0; i < 200; i++) d["k" + i] = i; for (let i = 0; i < 150; i++) delete d["k" + i];
F.call(d);
console.log("dict", d[s], Object.keys(d).length);
// plain data own key present: overwrite
const q: any = {}; q[s] = 0; F.call(q); console.log("plain", q[s]);
