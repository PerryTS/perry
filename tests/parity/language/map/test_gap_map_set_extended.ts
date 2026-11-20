// Gap test: Map and Set extended operations
// Run: node --experimental-strip-types test_gap_map_set_extended.ts

// --- Map basic operations ---
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
m.set("c", 3);
console.log("map size:", m.size);
console.log("map get a:", m.get("a"));
console.log("map get b:", m.get("b"));
console.log("map has a:", m.has("a"));
console.log("map has z:", m.has("z"));

// --- Map.delete ---
m.delete("b");
console.log("after delete size:", m.size);
console.log("has b after delete:", m.has("b"));

// --- Map.clear ---
const m2 = new Map<string, number>();
m2.set("x", 10);
m2.set("y", 20);
m2.clear();
console.log("after clear size:", m2.size);

// --- Map from entries ---
const m3 = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
console.log("from entries size:", m3.size);
console.log("from entries a:", m3.get("a"));
console.log("from entries c:", m3.get("c"));

// --- Map.set overwrites ---
const m4 = new Map<string, number>();
m4.set("key", 1);
m4.set("key", 2);
console.log("overwrite:", m4.get("key"));
console.log("overwrite size:", m4.size);

// --- Map.forEach ---
const m5 = new Map<string, number>([["x", 10], ["y", 20], ["z", 30]]);
const forEachResult: string[] = [];
m5.forEach((value: number, key: string) => {
  forEachResult.push(key + "=" + value);
});
console.log("forEach:", forEachResult);

// --- Map.keys ---
const keys: string[] = [];
for (const k of m5.keys()) {
  keys.push(k);
}
console.log("keys:", keys);

// --- Map.values ---
const vals: number[] = [];
for (const v of m5.values()) {
  vals.push(v);
}
console.log("values:", vals);

// --- Map.entries ---
const entries: string[] = [];
for (const [k, v] of m5.entries()) {
  entries.push(k + ":" + v);
}
console.log("entries:", entries);

// --- Map with different value types ---
const mixed = new Map<string, any>();
mixed.set("num", 42);
mixed.set("str", "hello");
mixed.set("bool", true);
mixed.set("null", null);
mixed.set("undef", undefined);
mixed.set("arr", [1, 2, 3]);
console.log("mixed num:", mixed.get("num"));
console.log("mixed str:", mixed.get("str"));
console.log("mixed bool:", mixed.get("bool"));
console.log("mixed null:", mixed.get("null"));
console.log("mixed undef:", mixed.get("undef"));

// --- Set basic operations ---
const s = new Set<number>();
s.add(1);
s.add(2);
s.add(3);
s.add(2); // duplicate
console.log("set size:", s.size);
console.log("set has 1:", s.has(1));
console.log("set has 4:", s.has(4));

// --- Set.delete ---
s.delete(2);
console.log("after delete size:", s.size);
console.log("has 2 after delete:", s.has(2));

// --- Set.clear ---
const s2 = new Set<number>([10, 20, 30]);
console.log("before clear:", s2.size);
s2.clear();
console.log("after clear:", s2.size);

// --- Set from iterable ---
const s3 = new Set<number>([1, 2, 3, 2, 1]);
console.log("from iterable size:", s3.size);
console.log("from iterable has 1:", s3.has(1));
console.log("from iterable has 3:", s3.has(3));

// --- Set.forEach ---
const s4 = new Set<string>(["a", "b", "c"]);
const setForEach: string[] = [];
s4.forEach((value: string) => {
  setForEach.push(value);
});
console.log("set forEach:", setForEach);

// --- Set.values ---
const setVals: string[] = [];
for (const v of s4.values()) {
  setVals.push(v);
}
console.log("set values:", setVals);

// --- Set with strings ---
const strSet = new Set<string>();
strSet.add("hello");
strSet.add("world");
strSet.add("hello"); // duplicate
console.log("string set size:", strSet.size);
console.log("string set has hello:", strSet.has("hello"));
console.log("string set has foo:", strSet.has("foo"));

// --- Map/Set with number keys/values edge cases ---
const numMap = new Map<number, string>();
numMap.set(0, "zero");
numMap.set(-0, "neg-zero"); // -0 and 0 are same key
numMap.set(NaN, "nan");
numMap.set(NaN, "nan2"); // NaN === NaN for Map keys
numMap.set(Infinity, "inf");
console.log("numMap size:", numMap.size);
console.log("numMap 0:", numMap.get(0));
console.log("numMap NaN:", numMap.get(NaN));
console.log("numMap Inf:", numMap.get(Infinity));

// --- Large Map ---
const big = new Map<number, number>();
for (let i = 0; i < 1000; i++) {
  big.set(i, i * i);
}
console.log("big size:", big.size);
console.log("big get 0:", big.get(0));
console.log("big get 500:", big.get(500));
console.log("big get 999:", big.get(999));

// --- Large Set ---
const bigSet = new Set<number>();
for (let i = 0; i < 1000; i++) {
  bigSet.add(i);
}
console.log("bigSet size:", bigSet.size);
console.log("bigSet has 0:", bigSet.has(0));
console.log("bigSet has 999:", bigSet.has(999));
console.log("bigSet has 1000:", bigSet.has(1000));
