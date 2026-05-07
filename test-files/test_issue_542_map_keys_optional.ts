// Issues #542 / #543: Map iteration through `Map | undefined` parameters.
//
// codehz/ecs (v0.5.503 repro) stores entity-relation IDs as f64-sized
// negative bigints (`relation(comp, target)` encodes as
// `-(componentId * 2^42 + targetId)`). Several archetype helpers receive
// a `Map<EntityId, any> | undefined` parameter and iterate it with
// `for (const [k, v] of m)` after an early-return guard, expecting Map
// semantics. Pre-fix, perry's HIR for-of lowering only entered the Map
// fast path when `iterable_type` was the bare `Type::Generic { base:
// "Map", ... }`; an `Map<K, V> | undefined` parameter shows up as
// `Type::Union([Generic{Map}, Void])` and fell through to array
// iteration, which read garbage from the MapHeader bytes — producing
// either zero iterations (when the map's `size` lined up with index 0
// being out-of-range) or yielding swapped/garbage [k, v] pairs.
//
// `m.keys()` / `m.values()` / `m.entries()` had a parallel bug for
// any-typed receivers: the array-method fast path's catch-all folded
// the call to `Expr::ArrayKeys`, which then ran `js_array_keys` against
// a real MapHeader and returned `[0..N-1]`.
//
// This test exercises both paths via a Map-typed parameter.

const RELATION_BITS = -8796093023232; // -(2 * 2^42), outside i32 range

function iterateOptionalMap(m: Map<number, string> | undefined): string[] {
  if (!m) return [];
  const out: string[] = [];
  // for-of [k, v] destructure on Union<Map, undefined> after narrow.
  for (const [k, v] of m) {
    out.push(`${k}=${v}`);
  }
  return out;
}

function keysOptionalMap(m: Map<number, string> | undefined): number[] {
  if (!m) return [];
  // .keys() on Union<Map, undefined> after narrow.
  return [...m.keys()];
}

function valuesOptionalMap(m: Map<number, string> | undefined): string[] {
  if (!m) return [];
  return [...m.values()];
}

const m = new Map<number, string>();
m.set(RELATION_BITS, "first");
m.set(RELATION_BITS - 1024, "second");

// Static-typed call: parameter is `Map | undefined`.
const pairs = iterateOptionalMap(m);
console.log("pairs:", pairs.join(","));

const keys = keysOptionalMap(m);
console.log("keys:", keys.join(","));

const values = valuesOptionalMap(m);
console.log("values:", values.join(","));

// Undefined narrow produces empty.
console.log("undefined pairs:", iterateOptionalMap(undefined).length);
console.log("undefined keys:", keysOptionalMap(undefined).length);
