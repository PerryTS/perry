// Gap test: Map/Set .entries()/.keys()/.values() reached via the `any`-typed
// `Expr::ArrayEntries` catch-all in codegen.
// Run: node --experimental-strip-types test_gap_map_set_entries_dispatch.ts
//
// When the receiver's static type is lost (e.g. a `Map` read off an `any`-typed
// field or returned from an `any` function), codegen lowers `.entries()` to the
// Array fast path (`js_array_entries`). Before the fix, that helper
// reinterpreted the Map/Set buffer as an Array, producing garbage `[index,
// value]` pairs and segfaulting downstream on `pairs.length`. Effect's
// `FiberRefs.diff` does `for (const [k, v] of newValue.locals.entries())` where
// `locals` is a `Map`. (#321 effect Context/Layer.)

function getMap(): any {
  const m = new Map<any, any>();
  m.set("k1", [[1, "a"], [2, "b"]]);
  m.set("k2", [[3, "c"]]);
  return m;
}

const anyMap: any = getMap();

for (const [k, pairs] of anyMap.entries()) {
  console.log("map entries:", k, pairs.length, pairs[0][1]);
}
// map entries: k1 2 a
// map entries: k2 1 c

const mapKeys: any[] = [];
for (const k of anyMap.keys()) {
  mapKeys.push(k);
}
console.log("map keys:", mapKeys); // ['k1', 'k2']

const mapValLens: number[] = [];
for (const v of anyMap.values()) {
  mapValLens.push(v.length);
}
console.log("map value lengths:", mapValLens); // [2, 1]

function getSet(): any {
  const s = new Set<any>();
  s.add(10);
  s.add(20);
  s.add(30);
  return s;
}

const anySet: any = getSet();

for (const [a, b] of anySet.entries()) {
  console.log("set entries:", a, b);
}
// set entries: 10 10
// set entries: 20 20
// set entries: 30 30

const setVals: number[] = [];
for (const v of anySet.values()) {
  setVals.push(v);
}
console.log("set values:", setVals); // [10, 20, 30]

const setKeys: number[] = [];
for (const k of anySet.keys()) {
  setKeys.push(k);
}
console.log("set keys:", setKeys); // [10, 20, 30]
