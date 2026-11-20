// #10943 layer 2: a proof of KIND is not a proof of NO OWN OVERRIDE.
//
// ECMA-262 resolves `recv.m(a)` as `Get(recv, "m")` then `Call`. perry lowers
// a method call to a DIRECT native call (`js_map_get`, `js_date_*`,
// `js_array_*`, ...) whenever it can prove the receiver's KIND — and an own
// property that shadows the method leaves that kind proof entirely intact:
// `const m = new Map(); m.get = () => 1;` is still provably a Map.
//
// #10476 already fixed this for UNPROVEN receivers: their runtime kind picks
// the builtin or the universal dispatcher, "which finds an own or inherited
// user method". The proven-receiver branch never got the same treatment.
//
// EVERY CALL BELOW PASSES AN ARGUMENT. That is deliberate and it is the whole
// point of this file: the previous attempt's differential used zero-argument
// calls throughout and went green against a fix that only covered
// zero-argument calls. A call with an argument is the spelling real code
// writes, and it is the one specialised away from the dispatcher — the HIR
// fold at `lower/expr_call/local_array_methods.rs:948` is gated on
// `!args.is_empty()`, and codegen's `map_set.rs` arms on `args.len() == 1`/`2`.
//
// The receiver forms are spread on purpose too: a bare parameter is NOT a
// defence, because HIR monomorphisation gives the clone that receives a Map a
// concrete `Map` local type. The polymorphic row proves it — the same
// function is correct for a plain object and wrong for a Map in one program.
//
// Byte-identical to node is the contract.

function t(label, f) {
  try { console.log(label + "=" + f()); } catch (e) { console.log(label + "=throw:" + e.message); }
}

// --- Map: the proven local, the spelling everyone writes -------------------
const m1 = new Map(); m1.set("k", 1); m1.get = () => "own";
t("map.get proven-local", () => m1.get("k"));
const m2 = new Map(); m2.set("k", 1); m2.has = () => "own";
t("map.has proven-local", () => m2.has("k"));
const m3 = new Map(); m3.set = () => "own";
t("map.set proven-local", () => m3.set("k", 1));
const m4 = new Map(); m4.set("k", 1); m4.delete = () => "own";
t("map.delete proven-local", () => m4.delete("k"));

// --- Map through receiver forms that defeat a naive "is it a local" test ---
function mono(x) { return x.get("k"); }
const m5 = new Map(); m5.set("k", 1); m5.get = () => "own";
t("map.get monomorphic-param", () => mono(m5));

function poly(x) { return x.get("k"); }
const m6 = new Map(); m6.set("k", 1); m6.get = () => "own";
t("map.get poly-plain", () => poly({ get: () => "plain" }));
t("map.get poly-map", () => poly(m6));

function anyp(x: any) { return x.get("k"); }
const m7 = new Map(); m7.set("k", 1); m7.get = () => "own";
t("map.get any-annotated", () => anyp(m7));

const arr = [new Map()]; arr[0].set("k", 1); arr[0].get = () => "own";
t("map.get array-element", () => arr[0].get("k"));

function make() { const m = new Map(); m.set("k", 1); m.get = () => "own"; return m; }
t("map.get call-result", () => make().get("k"));

class Holder { m = new Map(); }
const h = new Holder(); h.m.set("k", 1); h.m.get = () => "own";
t("map.get class-field", () => h.m.get("k"));

// --- Set / Date / Array: the same proof, the same hole ---------------------
const s1 = new Set([1]); s1.has = () => "own";
t("set.has proven-local", () => s1.has(1));
const s2 = new Set(); s2.add = () => "own";
t("set.add proven-local", () => s2.add(1));
const d1 = new Date(0); d1.setHours = () => "own";
t("date.setHours proven-local", () => d1.setHours(3));
const a2 = [1, 2]; a2.indexOf = () => "own";
t("array.indexOf proven-local", () => a2.indexOf(2));
const a3 = [3, 1]; a3.slice = () => "own";
t("array.slice proven-local", () => a3.slice(0));

// --- the native method must still work when nothing shadows it -------------
t("map.get native", () => new Map([["k", "v"]]).get("k"));
t("map.has native", () => new Map([["k", 1]]).has("k"));
t("set.has native", () => new Set([1]).has(1));
t("date.setHours native", () => { const d = new Date(0); d.setHours(3); return d.getHours(); });
t("array.push native", () => { const a = [1]; a.push(2); return a.length; });
t("array.indexOf native", () => [1, 2].indexOf(2));
t("array.slice native", () => JSON.stringify([3, 1].slice(0)));

// --- a prototype override (not own) must still reach the subclass ----------
class MyMap extends Map { get(k) { return "sub"; } }
t("subclass.get override", () => new MyMap().get("k"));

// --- deleting the own property restores the native method ------------------
const m8 = new Map([["k", "native"]]); m8.get = () => "own";
t("map.get before-delete", () => m8.get("k"));
delete m8.get;
t("map.get after-delete", () => m8.get("k"));

// --- reflection already agrees the own property is there -------------------
t("map.get hasOwn", () => Object.prototype.hasOwnProperty.call(m1, "get"));
const a1h = [1]; a1h.push = () => "own";
t("array.push hasOwn", () => Object.prototype.hasOwnProperty.call(a1h, "push"));
t("map.get typeof", () => typeof m1.get);

// --- ARGUMENT POSITION -----------------------------------------------------
// The guarded node's builtin arm re-lowers the WHOLE node, arguments included.
// A depth-counted suppression covers them too, so a folded call nested in an
// argument emitted no diamond and ran its native helper, while the same call
// in statement position took the own method: the two arms of one diamond
// disagreeing for the same source. Found by review on #10958 and reproduced
// on that branch AND on main before the fix, which makes it #10943 surviving
// in a spelling this file did not contain. The suppression is now keyed on
// node identity, so only the node being re-lowered is skipped.
const m9 = new Map(); m9.set("k", "native"); m9.get = (k) => "own:" + k;
const m10 = new Map();
t("map.get in map.set argument", () => { m10.set("k", m9.get("k")); return m10.get("k"); });
const a4 = [];
t("map.get in array.push argument", () => { a4.push(m9.get("k")); return a4[0]; });
t("map.get in a nested argument", () => { const inner = new Map(); inner.set("x", m9.get("k")); return inner.get("x"); });
t("map.get twice in one argument list", () => { const mm = new Map(); mm.set(m9.get("a"), m9.get("b")); return mm.get("own:a"); });
t("map.get in a concat argument", () => { const mm = new Map(); mm.set("k", "<" + m9.get("k") + ">"); return mm.get("k"); });
const s9 = new Set(); s9.add("v"); s9.has = (v) => "own:" + v;
t("set.has in map.set argument", () => { const mm = new Map(); mm.set("k", s9.has("v")); return mm.get("k"); });
// the same shapes with NOTHING shadowed must stay native
const m11 = new Map(); m11.set("k", "native11");
const m12 = new Map();
t("native get in set argument", () => { m12.set("k", m11.get("k")); return m12.get("k"); });

// --- `push`: an own method beats the builtin on every tier (#11021) --------
// `push` is not guarded by a diamond: a diamond around it costs the inline
// store (+94 instructions per call, against +6 for `indexOf`). It does not
// need one. Every inline push tier's admission mask tests
// OBJ_FLAG_ARRAY_DESCRIPTORS, and every install of an array's own named
// property arms that bit, so an array that owns `push` always lands in a slow
// arm -- and each of the five slow arms now has an exit whose value is the
// METHOD's return, not a recomputed length, with nothing appended.
//
// The rows are spread over the receivers that select each tier: a module
// global and a function local (inline tier: realloc arm), a pre-growth alias
// (forwarded arm), a captured and a boxed binding (the local tail), a number[]
// (the numeric tier's fallback).
const p1 = [1]; p1.push = (x) => "own:" + x;
t("array.push own value", () => p1.push(9));
t("array.push own length", () => p1.length);
// the own method still runs when the value is discarded (side effects only).
const p4 = []; let p4seen = 0; p4.push = (x) => { p4seen = x; return 0; };
p4.push(7);
t("array.push own discarded", () => p4seen + "/" + p4.length);
t("array.push own function-local this", () => {
  const a = [1, 2]; a.push = function (x) { return this.length * 100 + x; };
  return a.push(5);
});
t("array.push own captured", () => {
  const a = [1]; a.push = (x) => "cap:" + x; const f = () => a.push(3); return f();
});
t("array.push own boxed", () => {
  let a = [1]; const g = () => { a = [2]; }; a.push = (x) => "boxed:" + x;
  const r = a.push(4); g(); return r;
});
t("array.push own number[]", () => {
  const a: number[] = [1.5, 2.5]; let s = 0;
  a.push = (x) => { s += x; return 0; };
  for (let i = 0; i < 3; i++) a.push(i + 0.5);
  return s + "/" + a.length;
});
t("array.push own installed after growth", () => {
  const a = [1]; for (let i = 0; i < 100; i++) a.push(i);
  a.push = (x) => "late:" + x; return a.push(0) + "/" + a.length;
});
t("array.push own through a pre-growth alias", () => {
  const a = [1]; const b = a; for (let i = 0; i < 100; i++) a.push(i);
  b.push = (x) => "alias:" + x; return a.push(0) + "/" + b.push(1) + "/" + a.length;
});
t("array.push own in a loop, discarded", () => {
  const a = []; let n = 0; a.push = (x) => { n += x; return -1; };
  for (let i = 0; i < 10; i++) a.push(i);
  return n + "/" + a.length;
});
t("array.push own via defineProperty", () => {
  const a = [1];
  Object.defineProperty(a, "push", { value: (x) => "dp:" + x, writable: true, configurable: true });
  return a.push(1);
});
t("array.push own accessor", () => {
  const a = [1]; let gets = 0;
  Object.defineProperty(a, "push", { get() { gets++; return (x) => "get:" + x; } });
  return a.push(1) + "/" + gets;
});
t("array.push own then deleted", () => {
  const a = [1]; a.push = () => "own"; const r1 = a.push(2);
  delete a.push; const r2 = a.push(3); return r1 + "/" + r2 + "/" + a.length;
});
// the bit means "some named property", not "an own push": an unrelated one
// must still take the builtin, through the same arm.
const p2 = [1]; p2.foo = 1;
t("array.push unrelated named prop", () => p2.push(2));
t("array.push unrelated named prop length", () => p2.length);
t("array.push unrelated named prop in a loop", () => {
  const a = [0]; a.foo = 1; for (let i = 0; i < 50; i++) a.push(i);
  return a.length + "/" + a[50];
});
// a BORROWED builtin is not a user method and must take the native arm.
const p3 = [1]; p3.push = Array.prototype.push;
t("array.push borrowed builtin", () => p3.push(2));
t("array.push borrowed builtin length", () => p3.length);
// a zero-argument push is a native call, guarded like the other call-only folds.
t("array.push own zero-argument", () => {
  const a = [1]; a.push = (...xs) => "zero:" + xs.length; return a.push() + "/" + a.length;
});
// Not covered here, because they do not lower through a guarded push:
// `a.push(x, y)` (HIR desugars it into one ArrayPush per argument, so an own
// method runs once per argument) and `a.push(...xs)` (ArrayPushSpread).
