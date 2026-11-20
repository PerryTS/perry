// #2770/#2771/#2772 — Map/Set/WeakMap/WeakSet constructors consume any
// iterable and validate init values; weak mutators reject primitives.

function err(fn: () => void): string {
  try {
    fn();
    return "no throw";
  } catch (e: any) {
    return e.name + ": " + e.message;
  }
}

// ---- Map constructor (#2770) ----
const m = new Map([
  ["a", 1],
  ["b", 2],
]);
console.log("map size", m.size, m.get("a"), m.get("b"));

const mClone = new Map(m);
console.log("map clone", mClone.size, mClone.get("a"));

const mFromSet = new Map(new Set([["s", 3]]));
console.log("map from set", mFromSet.size, mFromSet.get("s"));

const mShort = new Map([["short"]]);
console.log("map short", mShort.size, mShort.has("short"), mShort.get("short"));

const mEmptyPair = new Map([[]]);
console.log(
  "map empty pair",
  mEmptyPair.size,
  mEmptyPair.has(undefined),
  mEmptyPair.get(undefined),
);

console.log("map null", new Map(null).size);
console.log("map undefined", new Map(undefined).size);

console.log("map(5):", err(() => new Map(5 as any)));
console.log("map({a:1}):", err(() => new Map({ a: 1 } as any)));
console.log("map([1]):", err(() => new Map([1] as any)));

// ---- Set constructor (#2771) ----
console.log("set dedup", [...new Set([1, 2, 1])].join(","));
console.log("set from set", [...new Set(new Set([3, 4]))].join(","));
console.log("set from string", [...new Set("aba")].join(","));
console.log(
  "set from map",
  [...new Set(new Map([[1, "a"], [2, "b"]]))].map((v: any) => v.join(":")).join(","),
);
console.log("set null", new Set(null).size);
console.log("set undefined", new Set(undefined).size);
console.log("set(5):", err(() => new Set(5 as any)));
console.log("set({a:1}):", err(() => new Set({ a: 1 } as any)));

// ---- WeakMap / WeakSet (#2772) ----
const k1 = {};
const k2 = {};

console.log("weakmap arr", new WeakMap([[k1, "v1"]]).get(k1));
console.log("weakmap from map", new WeakMap(new Map([[k2, "v2"]])).get(k2));
console.log("weakset arr", new WeakSet([k1]).has(k1));
console.log("weakset from set", new WeakSet(new Set([k2])).has(k2));

// null / undefined init produce an empty but fully-functional collection.
const wmNull = new WeakMap<any, any>(null);
wmNull.set(k1, "n");
console.log("weakmap null works", wmNull.get(k1));
const wsUndef = new WeakSet<any>(undefined);
wsUndef.add(k1);
console.log("weakset undefined works", wsUndef.has(k1));

console.log("weakmap(5):", err(() => new WeakMap(5 as any)));
console.log("weakmap({a:1}):", err(() => new WeakMap({ a: 1 } as any)));
console.log("weakmap([[1,x]]):", err(() => new WeakMap([[1, "x"]] as any)));
console.log("weakmap([1]):", err(() => new WeakMap([1] as any)));
console.log("weakset(5):", err(() => new WeakSet(5 as any)));
console.log("weakset([1]):", err(() => new WeakSet([1] as any)));

const wm = new WeakMap<any, any>();
const ws = new WeakSet<any>();
console.log("weakmap.set(1):", err(() => wm.set(1 as any, "x")));
console.log("weakset.add(1):", err(() => ws.add(1 as any)));

// dynamic-variable primitive (not an AST literal)
const primKey: any = 7;
console.log("weakmap.set(var):", err(() => wm.set(primKey, "y")));
console.log("weakset.add(var):", err(() => ws.add(primKey)));

console.log("weakmap.has(1)", new WeakMap<any, any>().has(1 as any));
console.log("weakmap.delete(1)", new WeakMap<any, any>().delete(1 as any));
console.log("weakset.has(1)", new WeakSet<any>().has(1 as any));
