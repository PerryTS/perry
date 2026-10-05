// #11886 / #11932: `fn.call(...)`, `fn.apply(...)` and `fn.bind(...)` at hot
// method sites. The site's emitted hit runs the intrinsic only while
// %Function.prototype%'s CURRENT call/apply/bind slot holds it, so a patch
// the compiler cannot see (an alias of the prototype, a computed key, a
// write in the middle of a loop) is honoured on the very next call.

const out: string[] = [];
const N = 40;

// Untyped helpers: every call below goes through one method site each.
function viaCall(f: any, t: any, a: any, b: any): any { return f.call(t, a, b); }
function viaCall0(f: any): any { return f.call(); }
function viaCall1(f: any, t: any): any { return f.call(t); }
function viaCallMany(f: any, t: any): any { return f.call(t, 1, 2, 3, 4, 5, 6); }
function viaApply(f: any, t: any, args: any): any { return f.apply(t, args); }
function viaApply1(f: any, t: any): any { return f.apply(t); }
function viaBind(f: any, t: any, a: any): any { return f.bind(t, a); }

function hot<T>(label: string, fn: (i: number) => T): void {
  let last: any;
  for (let i = 0; i < N; i++) last = fn(i);
  out.push(label + "=" + (typeof last === "function" ? "fn" : String(last)));
}

// 1. Targets of every kind.
function plain(this: any, a: any, b: any) { return (this && this.k !== undefined ? this.k : "u") + ":" + a + ":" + b; }
function strictFn(this: any, a: any) { "use strict"; return (this === undefined ? "undef" : this === null ? "null" : typeof this) + ":" + a; }
function sloppyType(this: any) { return typeof this; }
const arrow = (a: any, b: any) => "arrow:" + a + ":" + b;
const obj: any = { k: "obj", m(a: any, b: any) { return this.k + ".m:" + a + ":" + b; } };
const other: any = { k: "other" };
class K {
  k = "K";
  m(a: any, b: any) { return this.k + ".K.m:" + a + ":" + b; }
  static s(this: any, a: any) { return "static:" + (this === K ? "K" : this && this.k) + ":" + a; }
}
const kInst = new K();
function restFn(this: any, ...xs: any[]) { return "rest:" + (this && this.k) + ":" + xs.join("|"); }
function argsFn(this: any) { return "args:" + arguments.length + ":" + Array.prototype.join.call(arguments, "|"); }
function manyParams(a: any, b: any, c: any, d: any, e: any, f: any) { return [a, b, c, d, e, f].map(String).join(","); }
function genFn(this: any, n: number) { return "gen:" + n; }
async function asyncFn(x: number) { return x * 2; }
function* gen(n: number) { yield n; yield n + 1; }
const bound = plain.bind(other, "B");
function makeAdder(n: number) { return function (this: any, x: number) { return n + x + (this && this.k ? 1000 : 0); }; }
const adders = [makeAdder(1), makeAdder(2), makeAdder(3)];

hot("plain", (i) => viaCall(plain, obj, i, "x"));
hot("plain-undef-this", (i) => viaCall(plain, undefined, i, "y"));
hot("plain-null-this", (i) => viaCall(plain, null, i, "y"));
hot("strict-undef", (i) => viaCall(strictFn, undefined, i, 0));
hot("strict-null", (i) => viaCall(strictFn, null, i, 0));
hot("strict-prim", (i) => viaCall(strictFn, 5, i, 0));
hot("strict-str", (i) => viaCall(strictFn, "s", i, 0));
hot("sloppy-type-obj", () => viaCall1(sloppyType, other));
function sloppyThis(this: any) { return typeof this + (this === globalThis ? ":global" : ""); }
hot("sloppy-prim", () => viaCall1(sloppyThis, 5));
hot("sloppy-str", () => viaCall1(sloppyThis, "s"));
hot("sloppy-undef", () => viaCall1(sloppyThis, undefined));
hot("sloppy-null", () => viaCall1(sloppyThis, null));
hot("arrow", (i) => viaCall(arrow, obj, i, "z"));
hot("method-own-this", (i) => viaCall(obj.m, obj, i, 1));
hot("method-other-this", (i) => viaCall(obj.m, other, i, 1));
hot("class-method", (i) => viaCall(kInst.m, other, i, 2));
hot("class-method-self", (i) => viaCall(K.prototype.m, kInst, i, 2));
hot("static", (i) => viaCall(K.s, other, i, 0));
hot("rest", (i) => viaCall(restFn, obj, i, "r"));
hot("arguments", (i) => viaCall(argsFn, obj, i, "a"));
hot("too-few", () => viaCall(manyParams, null, 1, 2));
hot("many", () => viaCallMany(manyParams, null));
hot("no-args", () => viaCall0(plain));
hot("bound", (i) => viaCall(bound, obj, i, "q"));
hot("generator", () => [...viaCall(gen, null, 7, 0)].join(","));
hot("genfn", (i) => viaCall(genFn, null, i, 0));
hot("adders", (i) => viaCall(adders[i % 3], i % 2 ? other : null, i, 0));
hot("builtin-hasOwn", (i) => viaCall(Object.prototype.hasOwnProperty, obj, i % 2 ? "k" : "zz", 0));
hot("builtin-slice", () => viaCall(Array.prototype.slice, [1, 2, 3, 4], 1, 3).join(","));
hot("builtin-toString", () => viaCall1(Object.prototype.toString, [1]));
let asyncOut = 0;
viaCall(asyncFn, null, 21, 0).then((v: number) => { asyncOut = v; });

// apply: arrays, array-likes, arguments, nullish.
hot("apply-array", (i) => viaApply(plain, obj, [i, "a"]));
hot("apply-like", (i) => viaApply(plain, other, { length: 2, 0: i, 1: "l" }));
hot("apply-short", (i) => viaApply(plain, other, [i]));
hot("apply-null", () => viaApply(plain, other, null));
hot("apply-undef", () => viaApply1(plain, other));
function forward(this: any, a: any, b: any) { return viaApply(plain, this, arguments); }
hot("apply-arguments", (i) => forward.call(obj, i, "f"));
hot("apply-rest", (i) => viaApply(restFn, obj, [i, 1, 2]));
hot("apply-builtin", () => viaApply(Math.max, null, [3, 9, 4]));
hot("apply-strict-prim", (i) => viaApply(strictFn, true, [i]));

// bind: closures, methods, arrows, bound, class constructors.
hot("bind-plain", (i) => viaBind(plain, obj, i)("b"));
hot("bind-arrow", (i) => viaBind(arrow, obj, i)("c"));
hot("bind-method", (i) => viaBind(obj.m, other, i)("d"));
hot("bind-bound", (i) => viaBind(bound, obj, i)());
const BoundK: any = viaBind(K, null, 0);
out.push("bind-class=" + new BoundK().k + ":" + (BoundK.name));
out.push("bind-name=" + viaBind(plain, null, 1).name + ":" + viaBind(plain, null, 1).length);

// Class constructors: call/apply throw, bind constructs.
for (const [label, f] of [["call", () => viaCall(K, {}, 1, 2)], ["apply", () => viaApply(K, {}, [])]] as const) {
  try { f(); out.push("class-" + label + "=no throw"); }
  catch (e) { out.push("class-" + label + "=" + (e as Error).constructor.name); }
}
// A non-callable receiver with a `call` key, and an own `call` on a function.
const fake: any = { call(t: any, a: any) { return "fake:" + a; } };
hot("fake-call", (i) => viaCall(fake, null, i, 0));
function shadowed() { return "body"; }
(shadowed as any).call = function (t: any, a: any) { return "own-call:" + a; };
hot("own-call", (i) => viaCall(shadowed, null, i, 0));
hot("own-call-sibling", (i) => viaCall(plain, obj, i, "s"));

// 2. Patches through an alias and computed keys: invisible to the compiler.
const log: string[] = [];
const FP: any = Object.getPrototypeOf(plain);
const kCall = ["c", "a", "l", "l"].join("");
const kApply = "app" + "ly";
const kBind = "bi" + "nd";
const saved = { call: FP[kCall], apply: FP[kApply], bind: FP[kBind] };
FP[kCall] = function (this: any, t: any, ...a: any[]) { log.push("call"); return Reflect.apply(this, t, a); };
FP[kApply] = function (this: any, t: any, a: any) { log.push("apply"); return Reflect.apply(this, t, a == null ? [] : a); };
FP[kBind] = function (this: any, t: any, ...pre: any[]) { log.push("bind"); const f = this; return (...a: any[]) => Reflect.apply(f, t, pre.concat(a)); };
hot("patched-call", (i) => viaCall(plain, obj, i, "p"));
hot("patched-call-arrow", (i) => viaCall(arrow, obj, i, "p"));
hot("patched-apply", (i) => viaApply(plain, obj, [i, "p"]));
hot("patched-bind", (i) => viaBind(plain, obj, i)("p"));
hot("patched-direct", (i) => (plain as any).call(other, i, "d"));
out.push("log=" + log.length + ":" + [...new Set(log)].join(","));
FP[kCall] = saved.call; FP[kApply] = saved.apply; FP[kBind] = saved.bind;
log.length = 0;
hot("restored-call", (i) => viaCall(plain, obj, i, "r"));
hot("restored-apply", (i) => viaApply(plain, obj, [i, "r"]));
hot("restored-bind", (i) => viaBind(plain, obj, i)("r"));
out.push("log-after-restore=" + log.length);

// 3. A patch applied in the middle of a hot loop takes effect on the next call.
let mid = "";
for (let i = 0; i < 60; i++) {
  if (i === 30) FP[kCall] = function (this: any) { return "mid-patched"; };
  if (i === 45) FP[kCall] = saved.call;
  const r = viaCall(plain, obj, i, "m");
  if (i === 29 || i === 30 || i === 44 || i === 45) mid += i + "=" + r + ";";
}
out.push("mid-loop=" + mid);

// 4. Swapping intrinsics between slots: `call` now runs `apply`'s semantics,
// both at a site primed before the swap and at one first reached after it.
FP[kCall] = saved.apply;
out.push("swapped=" + viaCall(plain, obj, ["s1", "s2"], "ignored"));
function viaCallSwapped(f: any, t: any, a: any, b: any): any { return f.call(t, a, b); }
hot("swapped-fresh-site", (i) => viaCallSwapped(plain, obj, [i, "w"], "ignored"));
FP[kCall] = saved.call;
out.push("unswapped=" + viaCall(plain, obj, "u1", "u2"));
hot("unswapped-fresh-site", (i) => viaCallSwapped(plain, obj, i, "w"));

// 5. A non-callable value in the slot is a TypeError.
FP[kCall] = 42;
try { out.push("noncallable=" + viaCall(plain, obj, 1, 2)); }
catch (e) { out.push("noncallable=" + (e as Error).constructor.name); }
FP[kCall] = saved.call;
out.push("after-noncallable=" + viaCall(plain, obj, 1, 2));

setTimeout(() => {
  out.push("async=" + asyncOut);
  console.log(out.join("\n"));
}, 0);
