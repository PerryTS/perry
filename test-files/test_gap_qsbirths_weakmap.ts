// Dynamic builtin construction and subclass construction share WeakMap semantics.
const W: any = WeakMap;
const k: any = {}, other: any = {};
const a: any = new W(), b: any = new W();
a.set(k, 7); b.set(k, 11);
console.log("birth", a !== b, a instanceof WeakMap, b instanceof W,
  Object.getPrototypeOf(a) === WeakMap.prototype, a.get(k), b.get(k), a.has(other));
console.log("iterable", new W([[k, 19], [other, 23]]).get(k), new W(null).has(k),
  new W(undefined).has(k));
let seen: any;
class X extends WeakMap<object, number> {
  constructor(entries?: any) { super(entries); seen = new.target; }
}
const x = new X([[k, 29]]);
console.log("subclass", x instanceof X, x instanceof WeakMap, Object.getPrototypeOf(x) === X.prototype,
  seen === X, x.get(k), x.set(other, 31) === x, x.get(other));
class Y extends X {}
const y = new Y([[k, 37]]);
console.log("derived-target", y instanceof Y, y instanceof X, y instanceof WeakMap,
  seen === Y, Object.getPrototypeOf(y) === Y.prototype, y.get(k));
