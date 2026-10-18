// Z2: a method site on a WeakMap/WeakSet receiver still on its birth layout
// calls the weak operation directly; every other receiver (own key, changed
// [[Prototype]], subclass, frozen, newTarget, patched prototype, ordinary
// objects with get/set/has methods) keeps the generic behaviour.

let sentinel: any = { s: 1 };

function useMap(m: any, k: object, v: any): string {
  const out: any[] = [];
  out.push(m.has(k));
  out.push(m.get(k));
  out.push(m.set(k, v) === m);
  out.push(m.has(k));
  out.push(m.get(k));
  out.push(m.delete(k));
  out.push(m.delete(k));
  out.push(m.has(k));
  return JSON.stringify(out);
}

function useSet(s: any, k: object): string {
  const out: any[] = [];
  out.push(s.has(k));
  out.push(s.add(k) === s);
  out.push(s.has(k));
  out.push(s.delete(k));
  out.push(s.has(k));
  return JSON.stringify(out);
}

// One `has`/`delete` site sees both families.
function hasBoth(c: any, k: object): boolean {
  return c.has(k);
}

// Split sites: the argument reads a module global another function writes.
function split(c: any): any {
  return c.get(sentinel);
}

function tryCall(f: () => any): string {
  try {
    return String(f());
  } catch (e: any) {
    return e.constructor.name;
  }
}

const k1 = {};
const k2 = {};
console.log("plain map", useMap(new WeakMap(), k1, 7));
console.log("plain set", useSet(new WeakSet(), k1));
const wm = new WeakMap<object, number>([[k1, 1]]);
const ws = new WeakSet<object>([k2]);
console.log("both", hasBoth(wm, k1), hasBoth(ws, k2), hasBoth(wm, k2), hasBoth(ws, k1));
console.log("split weak", split(new WeakMap([[sentinel, "w"]])));

// Ordinary objects with builtin-named methods (side-channel shape).
const channel = {
  m: new WeakMap<object, any>(),
  get(k: object) { return "ch:" + this.m.get(k); },
  set(k: object, v: any) { this.m.set(k, v); return "set"; },
  has(k: object) { return "has:" + this.m.has(k); },
  delete(k: object) { return "del"; },
  add(k: object) { return "add"; },
};
console.log("channel", useMap(channel, k1, 3), useSet(channel, k1));
console.log("split channel", split(channel));
channel.m.set(sentinel, "s");
console.log("split channel 2", split(channel));
console.log("split null", tryCall(() => split(null)));
console.log("split prim", tryCall(() => split(42)));

// Own key on the instance: off the birth layout.
const own: any = new WeakMap();
own.get = (k: any) => "own-get";
console.log("own key", useMap(own, k1, 5));
const ownSet: any = new WeakSet();
ownSet.has = () => "own-has";
console.log("own set key", useSet(ownSet, k1), hasBoth(ownSet, k1));

// Changed [[Prototype]].
const reproto: any = new WeakMap();
Object.setPrototypeOf(reproto, {
  get: () => "proto-get",
  set: () => "proto-set",
  has: () => "proto-has",
  delete: () => "proto-del",
});
console.log("reproto", useMap(reproto, k1, 1));
const nullProto: any = new WeakMap();
Object.setPrototypeOf(nullProto, null);
console.log("null proto", tryCall(() => nullProto.get(k1)));

// Subclass instance.
class Sub extends WeakMap<object, any> {
  get(k: object) { return "sub:" + super.get(k); }
}
console.log("subclass", useMap(new Sub(), k1, 9));
class SubSet extends WeakSet<object> {
  has(k: object) { return super.has(k); }
}
console.log("subset", useSet(new SubSet(), k2));

// Reflect.construct with another newTarget.
function NT() {}
NT.prototype = { get: () => "nt-get", has: () => "nt-has", set: () => "nt-set", delete: () => "nt-del" };
const nt: any = Reflect.construct(WeakMap, [], NT);
console.log("newTarget", useMap(nt, k1, 2));

// Frozen / non-extensible collections keep working.
const frozen = Object.freeze(new WeakMap<object, number>());
console.log("frozen", useMap(frozen, k1, 4));
const sealedSet = Object.preventExtensions(new WeakSet<object>());
console.log("sealed set", useSet(sealedSet, k2));

// Wrong family: a WeakSet has no get/set, a WeakMap no add.
console.log("ws.get", tryCall(() => (new WeakSet() as any).get(k1)));
console.log("wm.add", tryCall(() => (new WeakMap() as any).add(k1)));
console.log("bad key", tryCall(() => new WeakMap().set(1 as any, 1)));
console.log("bad add", tryCall(() => new WeakSet().add("s" as any)));
console.log("prim get", new WeakMap().get(1 as any), new WeakSet().has(null as any));

// Many entries under allocation pressure: the birth word survives moves.
const big = new WeakMap<object, number>();
const keys: object[] = [];
let sum = 0;
for (let i = 0; i < 20000; i++) {
  const k = { i, pad: [i, i + 1, i + 2] };
  keys.push(k);
  big.set(k, i);
  if (big.has(keys[i >> 1])) sum += big.get(keys[i >> 1])!;
}
console.log("big", sum);

// Patched prototypes after the sites ran: the same sites must see the patch.
const keep = new WeakMap<object, number>([[k1, 11]]);
const keepSet = new WeakSet<object>([k1]);
const origGet = WeakMap.prototype.get;
(WeakMap.prototype as any).get = function (k: object) { return "patched:" + origGet.call(this, k); };
console.log("patched get", useMap(keep, k1, 12), split(new WeakMap([[sentinel, "p"]])));
(WeakSet.prototype as any).has = function () { return "patched-has"; };
console.log("patched has", useSet(keepSet, k1), hasBoth(keepSet, k1), hasBoth(keep, k1));
(Object.prototype as any).add = function () { return "objproto-add"; };
console.log("objproto", tryCall(() => (new WeakMap() as any).add(k1)), useSet(new WeakSet(), k2));

// Direct calls on locals bound to `new WeakMap()` / `new WeakSet()` (no
// name-keyed fold: the site proves the receiver).
const dm = new WeakMap<object, any>();
dm.set(k1, "d");
(dm as any).get = () => "own-direct";
console.log("direct own", dm.get(k1), dm.has(k1));
const dp = new WeakMap<object, any>([[k1, 1]]);
Object.setPrototypeOf(dp, { get: () => "dp-proto", has: () => "dp-has" });
console.log("direct proto", dp.get(k1), dp.has(k1));
const dk = new WeakMap<object, number>([[k1, 21]]);
console.log("direct patched", dk.get(k1), dk.has(k1), dk.set(k2, 1) === dk, dk.delete(k2));
const ds = new WeakSet<object>();
console.log("direct set", ds.add(k1) === ds, ds.has(k1), ds.delete(k1), ds.has(k1));
let z: any = new WeakMap();
function reuse() { const z = new Map<string, number>(); z.set("s", 1); return z.get("s"); }
console.log("name reuse", reuse(), tryCall(() => z.set("prim", 1)));
