// Prototype adapters and folded adapters share one receiver proof.
const key: any = {};
const map: any = new WeakMap();
const set: any = new WeakSet();
const get = map.get.bind(map);
const put = map.set.bind(map);
const has = map.has.bind(map);
const drop = map.delete.bind(map);
const callGet = Function.prototype.call.bind(WeakMap.prototype.get);
put(key, 17);
console.log("bound", get(key), has(key), callGet(map, key), drop(key), has(key));
console.log("unbound", WeakMap.prototype.set.call(map, key, 19) === map,
  WeakMap.prototype.get.call(map, key), WeakMap.prototype.has.call(map, key));
const detached = map.get;
try { detached(key); console.log("detached", "accepted"); }
catch (e: any) { console.log("detached", e.name); }
for (const k of [null, undefined, true, 1, "key", 1n, Symbol.for("wmap-registered")]) {
  try { put(k, 1); console.log("invalid map", "accepted"); }
  catch (e: any) { console.log("invalid map", e.name); }
  try { set.add(k); console.log("invalid set", "accepted"); }
  catch (e: any) { console.log("invalid set", e.name); }
  console.log("lookup", get(k), has(k), drop(k), set.has(k), set.delete(k));
}
for (const k of [Symbol("local"), Symbol.iterator, [], function f() {}, {}]) {
  put(k, 23); set.add(k);
  console.log("held", get(k), has(k), set.has(k), drop(k), set.delete(k));
}
for (const receiver of [new Map(), new Set(), set, {}, WeakMap.prototype, Object.create(WeakMap.prototype)]) {
  for (const method of [WeakMap.prototype.get, WeakMap.prototype.set, WeakMap.prototype.has, WeakMap.prototype.delete]) {
    try { method.call(receiver, key, 1); console.log("map brand", "accepted"); }
    catch (e: any) { console.log("map brand", e.name); }
  }
}
for (const method of [WeakSet.prototype.add, WeakSet.prototype.has, WeakSet.prototype.delete]) {
  try { method.call(map, key); console.log("set brand", "accepted"); }
  catch (e: any) { console.log("set brand", e.name); }
}
class Child extends WeakMap {}
const child: any = new Child();
WeakMap.prototype.set.call(child, key, 29);
child.extra = "shape transition";
Object.setPrototypeOf(child, null);
Object.freeze(child);
console.log("subclass shape", WeakMap.prototype.get.call(child, key),
  WeakMap.prototype.set.call(child, key, 31) === child, WeakMap.prototype.get.call(child, key));
