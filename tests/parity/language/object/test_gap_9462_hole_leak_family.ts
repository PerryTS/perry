// #9462 — the hole-leak family: perry's internal empty-slot sentinel
// (`TAG_HOLE`) escaping into user-visible values, plus the value-decoding
// ladders that had no arm for it. Byte-compared against
// `node --experimental-strip-types`.
//
// `TAG_HOLE`'s bit pattern IS a NaN, so any ladder that falls through to
// "must be a regular number" turns a hole into `NaN` / `"number"`:
//
//   1. LEAK SOURCE — `js_array_get_f64`'s Set/Map arm read the raw element
//      buffer with no hole translation (the plain-array arm has it) and bounded
//      the read by the LIVE count `size` while raw slots run `0..used`. After
//      any `.delete()` it therefore handed back the tombstone itself.
//   2. `classify_value_typeof` had no hole arm: `typeof` said `"number"`.
//   3. `js_jsvalue_to_string` had none either: `String(hole)` was `"NaN"`, and
//      template interpolation routes through the same helper.
//   4. `console.table` rendered hole cells as `NaN` — and node omits the hole's
//      whole COLUMN, so the header derivation needed the same treatment.
//   5. `console.table` on an object with a deleted key printed `b | NaN`:
//      the key list was compacted without keeping each key's slot index, and
//      the fields were then read back by the compacted position.
//
// A note on why the Set/Map rows below are phrased as an invariant rather than
// printed raw: perry deliberately exposes a Set/Map's live elements through the
// same array-like indexed dispatch its `length` uses (`js_array_length` on a
// Set answers `size`), while in node a Set is simply not indexable. `view[0]`
// therefore differs by design and is not a parity question. What both must
// agree on is that no INTERNAL sentinel ever reaches user code — which is
// exactly what unfixed main violated, three ways at once.

// Reports `clean` in node for every value below (a Set is not indexable, so
// every read is `undefined`), and on unfixed perry main reported
// `SENTINEL-ESCAPED` — `typeof` "number", `String()` "NaN", `${}` "NaN".
function report(label: string, v: unknown): void {
  const escaped =
    (typeof v === "number" && Number.isNaN(v)) ||
    String(v) === "NaN" ||
    `${v}` === "NaN";
  console.log(label, escaped ? "SENTINEL-ESCAPED" : "clean");
}

// --- 1. the leak source: Set/Map tombstones ---

const liveSet = new Set<number>([1, 2, 3]);
liveSet.delete(1);
const setView = liveSet as unknown as number[];
report("set-tombstone-slot0", setView[0]);
report("set-tombstone-slot1", setView[1]);
report("set-tombstone-past-end", setView[5]);

const anySet: any = new Set<number>([10, 20, 30]);
anySet.delete(10);
report("set-tombstone-any", anySet[0]);

const liveMap = new Map<string, number>([["a", 1], ["b", 2]]);
liveMap.delete("a");
const mapView = liveMap as unknown as number[];
report("map-tombstone-slot0", mapView[0]);

const anyMap: any = new Map<string, number>([["x", 1], ["y", 2]]);
anyMap.delete("x");
report("map-tombstone-any", anyMap[0]);

// Through a typed parameter, so the read takes the specialized array lane.
function readAt(a: number[], i: number): unknown {
  return a[i];
}
const paramSet = new Set<number>([7, 8, 9]);
paramSet.delete(7);
report("set-tombstone-param", readAt(paramSet as unknown as number[], 0));

// --- 2. the second leak source, found while checking which downstream
//        symptoms survived the first: `Array.prototype.pop` ---
// The dense fast path DECLINES on a hole and falls through to the generic
// arm, which returned the raw slot. `[1, ,].pop()` therefore answered
// `typeof "number"`, `String()` `"NaN"`, and `!== undefined` — the exact
// shape #536 already fixed once in this same function for the empty-array
// case. Every hole flavour reaches it. This is the only user-reachable
// producer left after the Set/Map arm was fixed (swept: destructuring, rest,
// spread, at, slice, concat, flat, shift, splice, find/findLast, map, filter,
// indexOf, includes, join, sort, reduce, forEach, for-of, `in`,
// Object.values/entries/assign, object spread, JSON.stringify, Array.from,
// structuredClone, with/toSorted/toReversed/toSpliced, copyWithin, reverse,
// fill, entries/values iterators — all already translate).

const poppedTrailing = [1, ,].pop();
console.log("pop-typeof", typeof poppedTrailing);
console.log("pop-string", String(poppedTrailing));
console.log("pop-template", `${poppedTrailing}`);
console.log("pop-eq-undefined", poppedTrailing === undefined);
console.log("pop-is-nan", Number.isNaN(poppedTrailing as unknown as number));
console.log("pop-inspect", poppedTrailing);
console.log("pop-sized", String(new Array(3).pop()));
console.log("pop-sized-typeof", typeof new Array(1).pop());
const popDeleted = [1, 2, 3];
delete popDeleted[2];
const poppedDeleted = popDeleted.pop();
console.log("pop-deleted", String(poppedDeleted), typeof poppedDeleted);
console.log("pop-empty", String(([] as number[]).pop()));
console.log("pop-normal", String([1, 2].pop()));

// --- 3. array-hole producers: typeof / String / template ---
// These already answered `undefined` before the fix (the plain-array arm of
// `js_array_get_f64` translates the sentinel) and must not move.

const literalHoles = [1, , 3];
console.log("literal-typeof", typeof literalHoles[1]);
console.log("literal-string", String(literalHoles[1]));
console.log("literal-template", `${literalHoles[1]}`);
console.log("literal-value", literalHoles[1]);

const sized = new Array(3);
console.log("new-array-typeof", typeof sized[0]);
console.log("new-array-string", String(sized[0]));
console.log("new-array-template", `${sized[0]}`);
console.log("new-array-inspect", sized);

const deleted: any = { a: 1, b: 2 };
delete deleted.a;
console.log("delete-typeof", typeof deleted.a);
console.log("delete-string", String(deleted.a));
console.log("delete-template", `${deleted.a}`);
console.log("delete-inspect", deleted);
console.log("delete-keys", Object.keys(deleted).join(","));

const deletedElement = [1, 2, 3];
delete deletedElement[1];
console.log("delete-element-typeof", typeof deletedElement[1]);
console.log("delete-element-string", String(deletedElement[1]));
console.log("delete-element-inspect", deletedElement);

// --- 4. console.table on holey rows ---
// Node derives the columns from the UNION of each row's OWN keys, and a hole
// is not an own key: `[[1, , 3]]` has columns `0` and `2`, never a middle
// column holding `NaN`.

console.log("-- table holey row");
console.table([[1, , 3]]);
console.log("-- table hole in one of two rows");
console.table([[1, 2, 3], [4, , 6]]);
console.log("-- table all-hole row");
console.table([new Array(3)]);
console.log("-- table hole column order");
console.table([[1, , 3], [4, 5, 6]]);
console.log("-- table mixed primitive + holey row");
console.table([1, [2, , 4]]);
console.log("-- table sparse assignment");
const sparse = new Array(3);
sparse[1] = 5;
console.table([sparse]);
console.log("-- table holey primitives");
console.table([1, , 3]);

// --- 5. console.table on tombstoned objects ---

console.log("-- table deleted first key");
const rowObject: any = { a: 1, b: 2 };
delete rowObject.a;
console.table(rowObject);
console.log("-- table deleted middle key");
const threeKeys: any = { a: 1, b: 2, c: 3 };
delete threeKeys.b;
console.table(threeKeys);
console.log("-- table deleted nested row");
const nestedRows: any = { r1: { x: 1, y: 2 }, r2: { x: 3, y: 4 } };
delete nestedRows.r1;
console.table(nestedRows);
console.log("-- table row array with deleted key");
const partialRow: any = { a: 1, b: 2 };
delete partialRow.a;
console.table([partialRow, { a: 9, b: 8 }]);

// --- 6. collection tombstones through console.table ---
console.log("-- table set after delete");
const tableSet = new Set([1, 2, 3]);
tableSet.delete(1);
console.table(tableSet);
console.log("-- table map after delete");
const tableMap = new Map<string, number>([["a", 1], ["b", 2]]);
tableMap.delete("a");
console.table(tableMap);

// --- 7. the Map-parameter guard ---
// The guard's own accept/deopt verdict is not observable from TypeScript — it
// only decides whether a specialized clone runs — so it is asserted directly in
// `param_type_guard.rs`'s unit tests
// (`a_tombstoned_entry_does_not_deopt_a_collection_parameter`). What belongs
// here is the behavioural pin: the RESULT must be identical whichever clone is
// chosen, before and after a `.delete()`.
function sumMap(m: Map<string, number>): number {
  let total = 0;
  for (const v of m.values()) {
    total += v;
  }
  return total;
}
function joinSet(s: Set<number>): string {
  const out: number[] = [];
  for (const v of s) {
    out.push(v);
  }
  return out.join(",");
}
const guardedMap = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
console.log("map-param-before-delete", sumMap(guardedMap));
guardedMap.delete("a");
console.log("map-param-after-delete", sumMap(guardedMap));
console.log("map-param-size", guardedMap.size);
const guardedSet = new Set<number>([1, 2, 3]);
console.log("set-param-before-delete", joinSet(guardedSet));
guardedSet.delete(1);
console.log("set-param-after-delete", joinSet(guardedSet));

// --- 8. inspect controls (#9461 must not move) ---
console.log("inspect-set-after-delete", liveSet);
console.log("inspect-map-after-delete", liveMap);
console.log("inspect-holes", [1, , 3]);
console.log("inspect-new-array", new Array(3));
