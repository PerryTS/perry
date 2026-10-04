// #11826: module-level `let`/`const` have a temporal dead zone. A read
// before the declarator throws ReferenceError (Node: "Cannot access 'X'
// before initialization"), whether it is direct top-level code, a closure
// created early, a hoisted function, a class static block or a method called
// early. Compound and keyed forms throw before their key runs (f calls 0).
// After the declarator every form reads the value.
let calls = 0;
function f(): string { calls++; return "a"; }
function name(e: any): string { return e && e.constructor ? e.constructor.name : String(e); }
function probe(label: string, fn: () => any): void {
  calls = 0;
  try { const v = fn(); console.log(label, "no throw", typeof v); }
  catch (e) { console.log(label, name(e), "calls", calls); }
}
// direct reads at top level (no closure)
try { const y = A; console.log("direct no throw", y); } catch (e) { console.log("direct", name(e)); }
calls = 0; try { (A as any)[f()]; } catch (e) { console.log("key-read", name(e), calls); }
calls = 0; try { (A as any)[f()] = 1; } catch (e) { console.log("key-write", name(e), calls); }
calls = 0; try { (A as any)[f()] += 1; } catch (e) { console.log("key-compound", name(e), calls); }
try { (A as any).a += 1; } catch (e) { console.log("member-compound", name(e)); }
try { (A as any)["a"] += 1; } catch (e) { console.log("lit-compound", name(e)); }
try { (A as any).a; } catch (e) { console.log("member-read", name(e)); }
try { typeof A; console.log("typeof no throw"); } catch (e) { console.log("typeof", name(e)); }
try { L = 2; console.log("write no throw"); } catch (e) { console.log("let-write", name(e), (e as any).message); }
try { L++; console.log("update no throw"); } catch (e) { console.log("let-update", name(e)); }
try { L += 1; console.log("let compound no throw"); } catch (e) { console.log("let-compound", name(e)); }
try { console.log(U); } catch (e) { console.log("let-noinit", name(e)); }
try { console.log(N + 1); } catch (e) { console.log("num-const", name(e)); }
try { console.log(DP); } catch (e) { console.log("destructured", name(e)); }
try { console.log(E1); } catch (e) { console.log("export-const", name(e)); }
// closures created early
const early = () => A;
const earlyN = () => N;
probe("closure-early", early);
probe("closure-num-early", earlyN);
probe("hoisted-early", hoisted);
probe("hoisted-num-early", hoistedN);
probe("hoisted-indirect", viaHoisted);
// loops
for (let i = 0; i < 2; i++) { try { A; console.log("loop no throw"); } catch (e) { console.log("loop", i, name(e)); } }
let k = 0;
while (k < 2) { k++; try { L; } catch (e) { console.log("while", k, name(e)); } }
// switch at top level
switch (calls) { case 0: try { A; } catch (e) { console.log("switch", name(e)); } break; default: console.log("switch default"); }
// switch-case lexical jumped over
switch (1) { case 0: let sw = 1; break; case 1: try { console.log(sw); } catch (e) { console.log("switch-lexical", name(e)); } }
// labeled block
lbl: { try { A; } catch (e) { console.log("labeled", name(e)); break lbl; } console.log("not reached"); }
// class static block + static field before the declaration
class K {
  static s = (() => { try { return A; } catch (e) { return name(e); } })();
  static { try { A; console.log("static no throw"); } catch (e) { console.log("static-block", name(e)); } }
  m() { return A; }
}
console.log("static-field", K.s);
const kk = new K();
probe("method-early", () => kk.m());
// self reference in initializer (closure run during init)
const S: any = (() => { try { return S; } catch (e) { return name(e); } })();
console.log("self-init", S);
// an async function started early that reads after an await: the binding is
// initialized by then, so the read succeeds
async function lateRead() { await null; return A.a + N; }
const pendingRead = lateRead();
pendingRead.then((v) => console.log("async-after-await", v), (e) => console.log("async-after-await", name(e)));
// switch-case lexicals: entering at a later case skips the declaration
function inFn(k: number) {
  switch (k) { case 0: let sw2 = 1; console.log(sw2); break; case 1: try { console.log("fn no throw", sw2); } catch (e) { console.log("fn-switch-lexical", name(e)); } }
}
inFn(1);
inFn(0);
switch (1) { case 0: let q: any = { a: 1 }; break; case 1: try { console.log("top no throw", q.a); } catch (e) { console.log("top-switch-member", name(e)); } }
switch (0) { case 0: let ok = 3; case 1: console.log("fallthrough", ok); }
// the declarations
const A: any = { a: 1 };
let L = 1;
let U;
const N = 42;
const { p: DP } = { p: 7 };
export const E1 = 5;
function hoisted() { return A; }
function hoistedN() { return N; }
function viaHoisted() { return hoisted(); }
// after initialization everything reads the value
console.log("after", early().a, earlyN(), hoisted().a, hoistedN(), viaHoisted().a, kk.m().a, L, U, N, DP, E1);
const late = () => A.a + N;
console.log("late", late());
