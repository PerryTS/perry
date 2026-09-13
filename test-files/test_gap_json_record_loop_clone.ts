// #10123: the SHAPE-keyed element-shape versioned loop clone.
//
// #7480's clone keyed every proof on a compile-time class, so it could never
// fire for the one array shape record-processing code is actually written
// against: `JSON.parse`'d objects, which are class 0 with an ordinary birth
// ShapeId. The runtime now proves the exact ShapeId instead, and the clone's
// preheader resolves each tracked property's inline slot against it.
//
// Three things are new and every one of them is a MISCOMPILE if it is wrong,
// not a slow path, so each gets cases here rather than only a codegen unit
// test:
//
//   * the per-element residual no longer requires `GC_OBJ_TYPED_LAYOUT_INTACT`
//     (a parsed record never has it), so the loaded word is a NaN-boxed
//     JSValue and is tag-tested as a Number per read;
//   * `rows[7]` and `const d = i % n; rows[d]` are admitted as indices, each
//     with its own preheader bounds obligation;
//   * the array head is repaired BEFORE the `GC_TYPE_ARRAY` brand, so a lazy
//     JSON array is materialized instead of rejected.

function buildRecords(count: number, pad: string): string {
  const parts: string[] = [];
  for (let i = 0; i < count; i++) {
    parts.push(
      '{"id":' + i + ',"name":"user_' + i + '","active":' +
        (i % 2 === 0 ? "false" : "true") + ',"score":' + (i * 1.5) +
        ',"note":"' + pad + '"}',
    );
  }
  return "[" + parts.join(",") + "]";
}

// `repeat`: a CONSTANT index. The trip count says nothing about index 7, so
// the preheader owes its own `length > 7`.
function repeatSum(rows: any, count: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) sum += rows[7].id;
  return sum;
}

// `sequential`: the derived `i % n` index, lowered to one `srem` in the clone.
function sequentialSum(rows: any, count: number, n: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const index = i % n;
    sum += rows[index].id;
  }
  return sum;
}

// The original counter-indexed shape, now over an untyped receiver.
function scanSum(rows: any, count: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) sum += rows[i].id;
  return sum;
}

// `+` on a possibly-non-numeric field: JavaScript switches to string
// concatenation the moment one `id` is a string, which is exactly what the
// clone's per-read Number tag test has to preserve.
function concatIds(rows: any, count: number): string {
  let out = "";
  for (let i = 0; i < count; i++) out += String(rows[i].id) + ",";
  return out;
}

// ---------------------------------------------------------------------------
// 1. A LAZY parsed array (a top-level array over 1 KB is returned as a lazy
//    header, which the preheader's brand test rejected before the repair was
//    moved ahead of it).
// ---------------------------------------------------------------------------
const lazyText = buildRecords(64, "0123456789abcdef0123456789abcdef");
console.log("lazy-bytes-over-1k:", lazyText.length > 1024);
const lazy: any = JSON.parse(lazyText);
const lazyLength: number = lazy.length;

console.log("lazy-repeat:", repeatSum(lazy, 50));
console.log("lazy-repeat-again:", repeatSum(lazy, 50));
console.log("lazy-sequential:", sequentialSum(lazy, 200, lazyLength));
console.log("lazy-scan:", scanSum(lazy, lazyLength));
console.log("lazy-scan-again:", scanSum(lazy, lazyLength));

// ---------------------------------------------------------------------------
// 2. An EAGER parsed array (under 1 KB), the same loops.
// ---------------------------------------------------------------------------
const eagerText = buildRecords(6, "x");
console.log("eager-bytes-under-1k:", eagerText.length < 1024);
const eager: any = JSON.parse(eagerText);
console.log("eager-repeat-index-in-range:", eager.length > 7);
console.log("eager-sequential:", sequentialSum(eager, 20, eager.length));
console.log("eager-scan:", scanSum(eager, eager.length));

// ---------------------------------------------------------------------------
// 3. HETEROGENEOUS key sets. One record with an extra key has a different
//    ShapeId, so the array-level proof must decline for the WHOLE array —
//    a shape says which slot holds `id`, and the second shape's may differ.
// ---------------------------------------------------------------------------
const heteroText =
  '[{"id":1,"name":"a"},{"id":2,"name":"b"},{"extra":9,"id":3,"name":"c"},' +
  '{"id":4,"name":"d"}]';
const hetero: any = JSON.parse(heteroText);
console.log("hetero-scan:", scanSum(hetero, hetero.length));
console.log("hetero-sequential:", sequentialSum(hetero, 12, hetero.length));

// A key set that is the same NAMES in a different ORDER is still a different
// shape, and its `id` sits at a different slot.
const reorderedText =
  '[{"id":1,"name":"a"},{"name":"b","id":2},{"id":3,"name":"c"}]';
const reordered: any = JSON.parse(reorderedText);
console.log("reordered-scan:", scanSum(reordered, reordered.length));

// ---------------------------------------------------------------------------
// 4. NON-NUMERIC field values. The array is perfectly homogeneous in SHAPE —
//    every record has exactly `{id, name}` — so the array-level proof holds
//    and only the per-read Number tag test stands between the clone and
//    reading a string pointer's bits as a double.
// ---------------------------------------------------------------------------
const stringIdText =
  '[{"id":1,"name":"a"},{"id":2,"name":"b"},{"id":"three","name":"c"},' +
  '{"id":4,"name":"d"}]';
const stringId: any = JSON.parse(stringIdText);
console.log("string-id-sum:", scanSum(stringId, stringId.length));
console.log("string-id-concat:", concatIds(stringId, stringId.length));

const nullIdText = '[{"id":1},{"id":null},{"id":3}]';
const nullId: any = JSON.parse(nullIdText);
console.log("null-id-sum:", scanSum(nullId, nullId.length));

const boolIdText = '[{"id":1},{"id":true},{"id":3}]';
const boolId: any = JSON.parse(boolIdText);
console.log("bool-id-sum:", scanSum(boolId, boolId.length));

const objIdText = '[{"id":1},{"id":{"n":2}},{"id":3}]';
const objId: any = JSON.parse(objIdText);
console.log("obj-id-concat:", concatIds(objId, objId.length));

// Fractional and negative values still read as plain doubles.
const floatText = '[{"id":-1.5},{"id":0.25},{"id":1e21}]';
const floats: any = JSON.parse(floatText);
console.log("float-ids:", scanSum(floats, floats.length));
console.log("float-concat:", concatIds(floats, floats.length));

// ---------------------------------------------------------------------------
// 5. REVOCATION after the proof was established. Each of these leaves the
//    array's length alone, so only the element-store funnel or the per-element
//    residual can catch it.
// ---------------------------------------------------------------------------
const mutated: any = JSON.parse(buildRecords(40, "pad"));
console.log("mutated-before:", scanSum(mutated, mutated.length));
mutated[5] = 123;
console.log("mutated-after-primitive:", scanSum(mutated, mutated.length));

const reshaped: any = JSON.parse(buildRecords(40, "pad"));
console.log("reshaped-before:", scanSum(reshaped, reshaped.length));
delete reshaped[9].name;
console.log("reshaped-after-delete:", scanSum(reshaped, reshaped.length));

const downgraded: any = JSON.parse(buildRecords(40, "pad"));
console.log("downgraded-before:", scanSum(downgraded, downgraded.length));
downgraded[11].id = "eleven";
console.log("downgraded-after:", concatIds(downgraded, 14));

const accessorised: any = JSON.parse(buildRecords(40, "pad"));
Object.defineProperty(accessorised[3], "id", {
  get() {
    return 99;
  },
  configurable: true,
});
console.log("own-accessor:", scanSum(accessorised, accessorised.length));

// Length changes retire the proof through the pinned `verified_len`.
const grown: any = JSON.parse(buildRecords(40, "pad"));
console.log("grown-before:", scanSum(grown, grown.length));
grown.push({ id: 1000, name: "extra", active: true, score: 0, note: "pad" });
console.log("grown-after:", scanSum(grown, grown.length));
grown.pop();
grown.length = 5;
console.log("grown-truncated:", scanSum(grown, grown.length));

// ---------------------------------------------------------------------------
// 6. BOUNDS. Each index form's obligation is discharged once in the preheader;
//    a form whose obligation cannot be met must take the slow clone and
//    observe ordinary JavaScript semantics.
// ---------------------------------------------------------------------------
const shortArr: any = JSON.parse('[{"id":1},{"id":2},{"id":3}]');
// `rows[7]` on a 3-element array: `undefined.id` throws, exactly as JS says.
try {
  console.log("short-repeat:", repeatSum(shortArr, 4));
} catch (err) {
  console.log("short-repeat-threw:", String(err).slice(0, 9));
}
// A modulus LARGER than the array: the derived index runs past the end.
try {
  console.log("modulus-past-length:", sequentialSum(shortArr, 8, 10));
} catch (err) {
  console.log("modulus-past-length-threw:", String(err).slice(0, 9));
}
// `i % 0` is NaN in JavaScript, and `rows[NaN]` is `undefined` — the clone
// must never turn this into an `srem` by zero.
try {
  console.log("modulus-zero:", sequentialSum(shortArr, 4, 0));
} catch (err) {
  console.log("modulus-zero-threw:", String(err).slice(0, 9));
}
// A modulus SMALLER than the array is in range and stays specialized.
console.log("modulus-under-length:", sequentialSum(shortArr, 9, 2));
// A trip count past the array's length with the counter index.
try {
  console.log("scan-past-length:", scanSum(shortArr, 5));
} catch (err) {
  console.log("scan-past-length-threw:", String(err).slice(0, 9));
}
// Empty array: the invariant declines a vacuous proof.
const emptyArr: any = JSON.parse("[]");
console.log("empty-scan:", scanSum(emptyArr, emptyArr.length));

// ---------------------------------------------------------------------------
// 7. RECEIVERS THAT ARE NOT PLAIN PARSED ARRAYS. Each must decline the clone
//    and still produce the right answer.
// ---------------------------------------------------------------------------
const literalRows: any = [
  { id: 1, name: "a" },
  { id: 2, name: "b" },
  { id: 3, name: "c" },
];
console.log("object-literal-scan:", scanSum(literalRows, literalRows.length));

class RowList extends Array<any> {}
const subclass: any = new RowList();
subclass.push({ id: 4, name: "d" });
subclass.push({ id: 5, name: "e" });
console.log("subclass-scan:", scanSum(subclass, subclass.length));

const nested: any = JSON.parse('{"rows":[{"id":1},{"id":2},{"id":3}]}');
console.log("nested-scan:", scanSum(nested.rows, nested.rows.length));

const stringsArr: any = JSON.parse('["a","b","c"]');
console.log("primitive-elements-concat:", concatIds(stringsArr, 3));

const nullElems: any = JSON.parse('[{"id":1},null,{"id":3}]');
try {
  console.log("null-element:", scanSum(nullElems, nullElems.length));
} catch (err) {
  console.log("null-element-threw:", String(err).slice(0, 9));
}

// ---------------------------------------------------------------------------
// 8. A MODULE-LEVEL parsed array read from inside a function — the
//    module-global arm of the preheader's repaired-head write-back.
// ---------------------------------------------------------------------------
const moduleRows: any = JSON.parse(buildRecords(48, "0123456789abcdef"));
const moduleLength: number = moduleRows.length;

function sumModuleRows(count: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const index = i % moduleLength;
    sum += moduleRows[index].score;
  }
  return sum;
}
console.log("module-global:", sumModuleRows(120));
console.log("module-global-again:", sumModuleRows(120));

// A second field on the same records, so the preheader resolves two slots.
function sumTwoFields(rows: any, count: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) sum += rows[i].id + rows[i].score;
  return sum;
}
console.log("two-fields:", sumTwoFields(moduleRows, moduleLength));

// ---------------------------------------------------------------------------
// 9. #10185: the LOOP-CARRIED index (`random`), the K-statement accumulator
//    fold, and the two non-numeric reads.
//
//    Every case below is a MISCOMPILE if it is wrong, not a slow path:
//
//      * the carried write-back is placed at the END of the iteration, so a
//        mid-iteration side exit leaves the real binding holding that
//        iteration's ENTRY value and the slow clone advances the recurrence
//        exactly once. A write-back at the update site double-applies it, and
//        every later index is silently a different (still valid) record;
//      * K accumulator statements fold to one store for the same reason: a
//        side exit from the third read must not leave the first two applied;
//      * `.length` and the ternary read a NaN-boxed word and assume a
//        representation, so each tag-tests it and side-exits otherwise.
// ---------------------------------------------------------------------------

// The benchmark's `random` mode, with the `const index = cursor` alias.
function randomSum(rows: any, count: number, n: number): number {
  let sum = 0;
  let cursor = 0;
  for (let i = 0; i < count; i++) {
    cursor = (cursor * 17 + 7) % n;
    const index = cursor;
    sum += rows[index].id;
  }
  return sum;
}

// The same recurrence subscripted DIRECTLY, with the carried value read after
// the loop — the clone owes a write-back, and this is what observes it.
function randomSumAndCursor(
  rows: any,
  count: number,
  n: number,
  start: number,
): string {
  let sum: any = 0;
  let cursor = start;
  for (let i = 0; i < count; i++) {
    cursor = (cursor * 17 + 7) % n;
    sum += rows[cursor].id;
  }
  return String(sum) + "|" + String(cursor);
}

// A multiplier large enough that `a * cursor` leaves the range an `f64`
// represents exactly. JavaScript evaluates this in doubles, so an i64
// recurrence would NOT agree — the matcher must decline and let the ordinary
// lowering run.
function randomSumWideMultiplier(
  rows: any,
  count: number,
  n: number,
): string {
  let sum = 0;
  let cursor = 3;
  for (let i = 0; i < count; i++) {
    cursor = (cursor * 600000000 + 7) % n;
    sum += rows[cursor].id;
  }
  return String(sum) + "|" + String(cursor);
}

// A recurrence with an even step, so a fractional carried value still lands
// on whole-number indices after the first update.
function randomSumEvenStep(
  rows: any,
  count: number,
  n: number,
  start: number,
): string {
  let sum = 0;
  let cursor = start;
  for (let i = 0; i < count; i++) {
    cursor = (cursor * 2 + 7) % n;
    sum += rows[cursor].id;
  }
  return String(sum) + "|" + String(cursor);
}

// The benchmark's `fields` mode: three accumulator statements over ONE
// element, one through a string and one through a boolean.
function fieldsSum(rows: any, count: number, n: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const index = i % n;
    sum += rows[index].id;
    sum += rows[index].name.length;
    sum += rows[index].active ? 1 : 0;
  }
  return sum;
}

const randomRows: any = JSON.parse(buildRecords(40, "pad"));
console.log("random-sum:", randomSum(randomRows, 200, randomRows.length));
console.log(
  "random-sum-again:",
  randomSum(randomRows, 200, randomRows.length),
);
console.log(
  "random-cursor-live-after:",
  randomSumAndCursor(randomRows, 200, randomRows.length, 0),
);
// An odd trip count observes a different point of the cycle than the run
// above, so the write-back is checked at two places rather than one.
console.log(
  "random-cursor-live-after-odd:",
  randomSumAndCursor(randomRows, 201, randomRows.length, 0),
);
// A modulus smaller than the array. 13 is chosen because `(17c + 7) % 13`
// walks six distinct indices from 0 — several plausible moduli make the
// recurrence a fixed point at 0, where every assertion below would hold of a
// loop that read one element forever.
console.log(
  "random-small-modulus:",
  randomSumAndCursor(randomRows, 50, 13, 0),
);
console.log(
  "random-modulus-one:",
  randomSumAndCursor(randomRows, 9, 1, 0),
);

// Entry values the preheader must reject: JS `%` returns a NEGATIVE remainder
// for a negative dividend, and a fractional index is not an index at all.
// Both take the slow clone and keep ordinary JavaScript semantics.
try {
  console.log(
    "random-negative-start:",
    randomSumAndCursor(randomRows, 4, 5, -3),
  );
} catch (err) {
  console.log("random-negative-start-threw:", String(err).slice(0, 9));
}
// A FRACTIONAL entry value. The step is even so every index the loop
// actually reads is a whole number — the divergence being checked is the
// preheader's, not the subscript's: if the clone materialized `0.5` as the
// i32 `0` it would start the recurrence one step early and every index would
// shift, which the sum and the final cursor both show.
console.log(
  "random-fractional-start:",
  randomSumEvenStep(randomRows, 6, 5, 0.5),
);
console.log(
  "random-integral-start-same-step:",
  randomSumEvenStep(randomRows, 6, 5, 0),
);
// `x % 0` is NaN, and `rows[NaN]` is `undefined`.
try {
  console.log("random-modulus-zero:", randomSumAndCursor(randomRows, 3, 0, 0));
} catch (err) {
  console.log("random-modulus-zero-threw:", String(err).slice(0, 9));
}
// A modulus past the array's length runs off the end.
try {
  console.log(
    "random-modulus-past-length:",
    randomSumAndCursor(randomRows, 30, 500, 0),
  );
} catch (err) {
  console.log("random-modulus-past-length-threw:", String(err).slice(0, 9));
}
console.log(
  "random-wide-multiplier:",
  randomSumWideMultiplier(randomRows, 25, 11),
);

// THE write-back test. The array is homogeneous in SHAPE, so the clone is
// entered — but one `id` is a string, so the per-read Number tag test side
// -exits mid-iteration and the slow clone re-runs it. If the recurrence had
// already been committed, the slow clone advances it a SECOND time and both
// the sum and the final cursor drift.
// `(17c + 7) % 5` walks 0 -> 2 -> 1 -> 4 -> 0, so index 2 — the string one —
// really is read. A modulus that skipped it would make this whole case
// vacuous while still printing a matching number.
const randomMixed: any = JSON.parse(
  '[{"id":0},{"id":1},{"id":"two"},{"id":3},{"id":4}]',
);
console.log(
  "random-side-exit:",
  randomSumAndCursor(randomMixed, 30, randomMixed.length, 0),
);
console.log(
  "random-side-exit-again:",
  randomSumAndCursor(randomMixed, 30, randomMixed.length, 0),
);

// The recurrence over a HETEROGENEOUS array declines at the array level.
console.log(
  "random-hetero:",
  randomSumAndCursor(hetero, 16, hetero.length, 0),
);

// ---------------------------------------------------------------------------
// 9b. The `fields` shape.
// ---------------------------------------------------------------------------

// SSO names (<= 5 bytes, packed into the NaN-box) and heap names in the SAME
// array — the shape is identical either way, and the `.length` decode has to
// handle both.
const mixedNames: any = JSON.parse(
  '[{"id":1,"name":"ab","active":true},' +
    '{"id":2,"name":"user_222","active":false},' +
    '{"id":3,"name":"","active":true},' +
    '{"id":4,"name":"abcde","active":false},' +
    '{"id":5,"name":"abcdef","active":true}]',
);
console.log("fields-sso-and-heap:", fieldsSum(mixedNames, 25, mixedNames.length));
console.log(
  "fields-sso-and-heap-again:",
  fieldsSum(mixedNames, 25, mixedNames.length),
);

// A NON-STRING `name`: `.length` of a number is `undefined`, so the sum goes
// NaN — and the clone must side-exit rather than decode a double as a string
// header. The fold is what keeps the earlier `id` from being applied twice.
const numberName: any = JSON.parse(
  '[{"id":1,"name":"ab","active":true},{"id":2,"name":7,"active":true},' +
    '{"id":3,"name":"cd","active":false}]',
);
console.log("fields-number-name:", fieldsSum(numberName, 9, numberName.length));

// A NULL `name`: `null.length` throws, exactly as JS says.
const nullName: any = JSON.parse(
  '[{"id":1,"name":"ab","active":true},{"id":2,"name":null,"active":true}]',
);
try {
  console.log("fields-null-name:", fieldsSum(nullName, 4, nullName.length));
} catch (err) {
  console.log("fields-null-name-threw:", String(err).slice(0, 9));
}

// NON-BOOLEAN `active` values. JS truthiness of `0`, `""`, `"no"` and `null`
// is a runtime question the clone is not allowed to guess: only the two
// boolean singletons are admitted and everything else side-exits.
const truthyActive: any = JSON.parse(
  '[{"id":1,"name":"ab","active":true},{"id":2,"name":"cd","active":0},' +
    '{"id":3,"name":"ef","active":"no"},{"id":4,"name":"gh","active":""},' +
    '{"id":5,"name":"ij","active":null},{"id":6,"name":"kl","active":false}]',
);
console.log(
  "fields-truthiness:",
  fieldsSum(truthyActive, 24, truthyActive.length),
);
console.log(
  "fields-truthiness-again:",
  fieldsSum(truthyActive, 24, truthyActive.length),
);

// Two accumulator statements where the SECOND read side-exits: the first must
// not be applied twice when the slow clone re-runs the iteration.
function twoStatementSum(rows: any, count: number, n: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const index = i % n;
    sum += rows[index].id;
    sum += rows[index].score;
  }
  return sum;
}
const secondFieldMixed: any = JSON.parse(
  '[{"id":1,"score":10},{"id":2,"score":"x"},{"id":3,"score":30}]',
);
console.log(
  "fold-side-exit:",
  twoStatementSum(secondFieldMixed, 9, secondFieldMixed.length),
);

// Heterogeneous shapes with the multi-read body.
console.log("fields-hetero:", fieldsSum(hetero, 12, hetero.length));

// ---------------------------------------------------------------------------
// 9c. Property names the CLASS-keyed arm denies. A parsed record's own data
//     property SHADOWS the prototype name it collides with, and the
//     shape-keyed preheader resolves it from the runtime shape table, so
//     `name` and `length` are ordinary inline slots here. `name` in
//     particular is what the access benchmark's own `fields` mode reads.
// ---------------------------------------------------------------------------
function ownNamedFields(rows: any, count: number, n: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    const index = i % n;
    sum += rows[index].length;
    sum += rows[index].size;
    sum += rows[index].name.length;
  }
  return sum;
}
const ownNames: any = JSON.parse(
  '[{"length":3,"size":4,"name":"ab"},{"length":5,"size":6,"name":"cdef"},' +
    '{"length":7,"size":8,"name":"ghijklm"}]',
);
console.log("own-named-fields:", ownNamedFields(ownNames, 12, ownNames.length));
console.log(
  "own-named-fields-again:",
  ownNamedFields(ownNames, 12, ownNames.length),
);
