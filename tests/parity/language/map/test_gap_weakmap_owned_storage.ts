import { Buffer } from "node:buffer"
const map: any = new WeakMap()
const set: any = new WeakSet()
const keys: any[] = [
  {}, [], function f() {}, class C {}, new URLSearchParams("a=b"),
  Buffer.from([1, 2]), new Uint8Array([3, 4]), new Proxy({}, {}),
  Symbol("local"), Symbol.iterator,
]
for (let i = 0; i < keys.length; i++) {
  console.log("set", i, map.set(keys[i], i) === map, set.add(keys[i]) === set)
  console.log("hit", i, map.has(keys[i]), map.get(keys[i]), set.has(keys[i]))
  console.log("delete", i, map.delete(keys[i]), set.delete(keys[i]), map.delete(keys[i]))
  map.set(keys[i], i + 10)
  set.add(keys[i])
  console.log("reset", i, map.get(keys[i]), set.has(keys[i]))
}
for (const key of [undefined, null, false, 12, NaN, "x", 1n, Symbol.for("registered")]) {
  try { map.set(key, 1); console.log("bad-map", "accepted") }
  catch (e: any) { console.log("bad-map", e.name) }
  try { set.add(key); console.log("bad-set", "accepted") }
  catch (e: any) { console.log("bad-set", e.name) }
  console.log("primitive-lookup", map.get(key), map.has(key), map.delete(key), set.has(key), set.delete(key))
}
const live = {}
class ChildMap extends WeakMap {}
class ChildSet extends WeakSet {}
const childMap = new ChildMap()
const childSet = new ChildSet()
childMap.set(live, 99)
childSet.add(live)
console.log("subclasses", childMap.get(live), childMap.has(live), childSet.has(live))
for (const receiver of [{}, WeakMap.prototype, Object.create(WeakMap.prototype), { ...map }, Object.assign({}, map), new Proxy(map, {})]) {
  try { console.log("brand", WeakMap.prototype.get.call(receiver, live)) }
  catch (e: any) { console.log("brand", e.name) }
}

// Internal storage must remain private when shape/prototype facts change.
const owned: any = new WeakMap()
owned.__perry_wk_entries = "user field"
owned.extra = 1
owned.set(live, 123)
Object.defineProperty(owned, "accessor", { get: () => 7, enumerable: true })
console.log("own fields", Object.keys(owned).join(","), owned.__perry_wk_entries)
Object.setPrototypeOf(owned, null)
console.log("changed prototype", WeakMap.prototype.get.call(owned, live), WeakMap.prototype.has.call(owned, live))
Object.freeze(owned)
WeakMap.prototype.set.call(owned, live, 124)
console.log("frozen storage", WeakMap.prototype.get.call(owned, live))
const frozenEmptyMap = Object.freeze(new WeakMap())
const frozenEmptySet = Object.freeze(new WeakSet())
frozenEmptyMap.set(live, 125)
frozenEmptySet.add(live)
console.log("frozen empty", frozenEmptyMap.get(live), frozenEmptySet.has(live))
const many: any[] = []
for (let i = 0; i < 4096; i++) { const k = {}; many.push(k); map.set(k, i) }
let sum = 0
for (let i = 0; i < many.length; i++) { sum += map.get(many[i]); if (i % 2 === 0) map.delete(many[i]) }
for (let i = 0; i < many.length; i += 2) map.set(many[i], i + 1)
console.log("many", sum, map.get(many[0]), map.get(many[4094]), map.get(many[4095]))

for (const wrong of [new WeakSet(), {}, Object.create(WeakMap.prototype), new Proxy(map, {})]) {
  try { WeakMap.prototype.get.call(wrong, live); console.log("wrong brand", "accepted") }
  catch (e: any) { console.log("wrong brand", e.name) }
}
for (const method of [WeakMap.prototype.has, WeakMap.prototype.delete]) {
  try { method.call(set, live); console.log("map/set brand", "accepted") }
  catch (e: any) { console.log("map/set brand", e.name) }
}
for (const method of [WeakSet.prototype.has, WeakSet.prototype.delete]) {
  try { method.call(map, live); console.log("set/map brand", "accepted") }
  catch (e: any) { console.log("set/map brand", e.name) }
}
