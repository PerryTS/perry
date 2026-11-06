// Regression test for v0.5.212: type predicates on lazy JSON.parse
// results must behave exactly like predicates on eager arrays. Each
// line below asserts the claim documented in docs/audit-lazy-json.md
// Sections 5.6, 5.7, 7 (correctness table).

// Build a blob > 1 KB so the lazy path fires under the auto-default
// (LAZY_MIN_BLOB_BYTES = 1024).
const items: any[] = [];
for (let i = 0; i < 3000; i++) {
  items.push({ id: i, n: "item_" + i });
}
const blob = JSON.stringify(items);

const parsed = JSON.parse(blob);

// Array.isArray: codegen indeterminate-type branch routes through
// js_array_is_array, which matches GC_TYPE_LAZY_ARRAY.
console.log("isArray:" + Array.isArray(parsed));

// instanceof: js_instanceof's CLASS_ID_ARRAY arm matches
// GC_TYPE_LAZY_ARRAY.
console.log("instanceof:" + (parsed instanceof Array));

// typeof: NaN-boxed pointer → "object" regardless of obj_type.
console.log("typeof:" + typeof parsed);

// Scalar and object comparisons must still return false — guard
// against an over-eager fix that made isArray return true for
// non-arrays.
const str = "not an array";
const obj: any = { length: 5 };
const num = 42;
console.log("str-isArray:" + Array.isArray(str));
console.log("obj-isArray:" + Array.isArray(obj));
console.log("num-isArray:" + Array.isArray(num));
console.log("null-isArray:" + Array.isArray(null));
console.log("undefined-isArray:" + Array.isArray(undefined));

// A regular eager-constructed array must still pass both predicates.
const eager = [1, 2, 3];
console.log("eager-isArray:" + Array.isArray(eager));
console.log("eager-instanceof:" + (eager instanceof Array));

// Lazy after .length — still reports as array.
const len = parsed.length;
console.log("after-length-isArray:" + Array.isArray(parsed));

// Lazy after indexed access — sparse cache populated, but still array.
const first = parsed[0];
console.log("after-index-isArray:" + Array.isArray(parsed));

// Lazy after force-materialize (via .map) — now backed by a real
// ArrayHeader, still array.
const mapped = parsed.map((x: any) => x.id);
console.log("after-map-isArray:" + Array.isArray(parsed));
console.log("map-result-isArray:" + Array.isArray(mapped));
