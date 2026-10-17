// Method calls on WeakMap/WeakSet receivers take the builtin entries
// (object model step 5, phase 2): a plain weak collection answers through its
// intrinsic prototype; an own property, a relinked [[Prototype]], a subclass,
// a patched prototype must not. Every
// receiver reaches its site untyped (`u`), as in a compiled package.
function u(x: any): any { return x; }
const k1 = {}, k2 = {}, k3 = {};
const wm = new WeakMap<object, number>();
u(wm).set(k1, 1).set(k2, 2);
const w = u(wm);
console.log(w.has(k1), w.get(k2), w.has(k3), w.get(k3), w.delete(k1), w.has(k1));
const ws = new WeakSet<object>();
const s = u(ws);
s.add(k1).add(k2);
console.log(s.has(k1), s.has(k3), s.delete(k2), s.has(k2));
// own property shadows the method
const own: any = new WeakMap();
own.get = (k: object) => "own:" + (k === k1);
console.log(u(own).get(k1), u(own).has(k1));
// relinked prototype
const relinked: any = new WeakMap();
Object.setPrototypeOf(relinked, { has: () => "proto-has" });
console.log(u(relinked).has(k1));
// subclass
class Tagged extends WeakMap<object, string> {
  get(k: object) { return "tag:" + super.get(k); }
}
const t = new Tagged();
u(t).set(k1, "x");
console.log(u(t).get(k1), u(t).has(k1));
// keys that are not objects
try { w.set(1, 1); } catch (e) { console.log("set prim:", (e as Error).constructor.name); }
console.log(w.get(1), w.has("x"));
// patched prototype (last: it withdraws the pristine fact)
const saved = WeakMap.prototype.get;
(WeakMap.prototype as any).get = function () { return "patched"; };
console.log(w.get(k2));
WeakMap.prototype.get = saved;
console.log(w.get(k2));
