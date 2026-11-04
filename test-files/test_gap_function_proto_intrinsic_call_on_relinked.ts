// Function.prototype.call / apply / bind invoked reflectively act on their
// `this` value's [[Call]], never by re-reading `call` / `apply` / `bind` off
// it: a function relinked by Object.setPrototypeOf to an object that defines
// its own call/apply/bind still runs its own body.
function t(a?: any, b?: any) {
  return "t-body:" + a + ":" + b;
}
const p = {
  call() { return "p.call"; },
  apply() { return "p.apply"; },
  bind() { return "p.bind"; },
};
Object.setPrototypeOf(t, p);

console.log("call", Function.prototype.call.call(t));
console.log("call args", Function.prototype.call.call(t, null, 1, 2));
console.log("apply", Function.prototype.apply.call(t, null, [3, 4]));
const bound = Function.prototype.bind.call(t, null, 5);
console.log("bind", typeof bound, bound(6));

// Held in a local, the intrinsics behave the same.
const C: any = Function.prototype.call;
const A: any = Function.prototype.apply;
const B: any = Function.prototype.bind;
console.log("alias call", C.call(t, null, 7));
console.log("alias apply", A.call(t, null, [8]));
console.log("alias bind", B.call(t, null)(9));
console.log("reflect", Reflect.apply(Function.prototype.call, t, [null, 10]));

// An own `call` property is shadowing too.
function u() { return "u-body"; }
(u as any).call = function () { return "own.call"; };
console.log("own", Function.prototype.call.call(u), (u as any).call());

// The brand checks still throw.
for (const f of [() => Function.prototype.call.call({}), () => Function.prototype.apply.call(1), () => Function.prototype.bind.call("s")]) {
  try { f(); console.log("no throw"); } catch (e) { console.log("throws", e instanceof TypeError); }
}
class K {}
try { Function.prototype.call.call(K); console.log("no throw"); } catch (e) { console.log("class throws", e instanceof TypeError); }
