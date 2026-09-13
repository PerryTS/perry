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
