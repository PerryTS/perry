// @ts-nocheck
// #10509: arguments objects share one shape per arity and callee kind, and
// every strict arguments object shares one restricted-`callee` accessor pair.
// Behaviour must stay per-object: sloppy aliasing, strict throwing `callee`,
// non-configurable `callee`, per-object mutation.
function sloppy(a, b) {
  arguments[0] = 10;
  b = 20;
  const r = [a, arguments[1], arguments.length, typeof arguments.callee];
  delete arguments[0];
  arguments[0] = 30;
  r.push(a, arguments[0]);
  return r.join(",");
}
console.log(sloppy(1, 2), sloppy(3, 4), sloppy(5));

function strictArgs() { "use strict"; return arguments; }
const s1 = strictArgs(1, 2), s2 = strictArgs(3, 4), s3 = strictArgs();
for (const s of [s1, s2, s3]) {
  try { s.callee; console.log("no throw"); } catch (e) { console.log("get", e.constructor.name); }
}
const d1 = Object.getOwnPropertyDescriptor(s1, "callee");
const d2 = Object.getOwnPropertyDescriptor(s2, "callee");
console.log(d1.get === d1.set, d1.get === d2.get, d1.enumerable, d1.configurable, "value" in d1);
try { Object.defineProperty(s1, "callee", { value: 1 }); console.log("redefined"); }
catch (e) { console.log("define", e.constructor.name); }
console.log(JSON.stringify(Object.getOwnPropertyNames(s1)), JSON.stringify(Object.keys(s2)));
// Per-object state stays per object although the shape is shared.
s1[0] = "x"; s1.extra = 1; delete s2[1];
console.log(JSON.stringify([s1[0], s1[1], s1.extra, s2[0], s2[1], s2.extra, s1.length, s2.length]));
console.log(Object.prototype.toString.call(s3), s3.length, Object.getPrototypeOf(s1) === Object.prototype);

// Many escaping objects across collections keep their own values.
function keep() { "use strict"; return arguments; }
const kept = [];
for (let i = 0; i < 20000; i++) { kept.push(keep(i, { k: i })); }
let sum = 0;
for (const a of kept) sum += a[0] + a[1].k + a.length;
console.log(sum);
const m = [];
function sloppyKeep(x) { x = x * 2; return arguments; }
for (let i = 0; i < 20000; i++) m.push(sloppyKeep(i));
let s = 0; for (const a of m) s += a[0];
console.log(s);
