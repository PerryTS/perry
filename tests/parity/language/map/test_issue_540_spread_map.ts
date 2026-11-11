// Issue #540: `[...map]` spread produced garbage when the source was a
// Map. `js_array_concat` (called from `Expr::ArraySpread` codegen)
// detected Sets via `is_registered_set` but had no parallel Map arm,
// so the runtime walked the MapHeader as if it were an ArrayHeader —
// reading `size` as `length` and pulling keys/values out of the wrong
// offsets, producing tiny denormal f64s. `Array.from(map)` worked
// because it routes through `js_array_clone` which already had the
// Map arm. Fix: add the matching arm to `js_array_concat`.

const map1 = new Map();
map1.set(-4398046512128, undefined);

const spread = [...map1];
const arrayFrom = Array.from(map1);

console.log("spread length:", spread.length);
console.log("arrayFrom length:", arrayFrom.length);
console.log("spread[0]:", spread[0]);
console.log("arrayFrom[0]:", arrayFrom[0]);

if (Array.isArray(spread[0])) {
  console.log("spread[0][0]:", spread[0][0]);
  console.log("spread[0][1]:", spread[0][1]);
}

// Multi-entry Map with mixed value types.
const map2 = new Map<string, number | string | null>();
map2.set("a", 1);
map2.set("b", "two");
map2.set("c", null);
const spread2 = [...map2];
console.log("multi length:", spread2.length);
for (const [k, v] of spread2) {
  console.log(`pair k=${k} v=${JSON.stringify(v)}`);
}

// Spread inside a larger literal — exercises the dest-array path that
// already has elements when the Map source flows in.
const spread3 = [0, ...map2, 99];
console.log("around length:", spread3.length);
console.log("around first:", spread3[0]);
console.log("around last:", spread3[spread3.length - 1]);

// `[...set]` regression — Set detection in js_array_concat must keep working.
const set1 = new Set([10, 20, 30]);
const spreadSet = [...set1];
console.log("set spread:", spreadSet[0], spreadSet[1], spreadSet[2]);

// Empty Map spread.
const empty = new Map();
const spreadEmpty = [...empty];
console.log("empty length:", spreadEmpty.length);
