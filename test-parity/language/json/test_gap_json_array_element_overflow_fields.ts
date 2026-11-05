// JSON.stringify of an array whose elements carry more properties than the
// object's INLINE SLOT allocation must emit every property, not just the ones
// that happen to sit in inline slots.
//
// Perry's array fast path builds one shape template from element 0 and reuses
// its pre-formatted key prefixes for every element. It sized that template
// from `min(keys_len, field_count)`. `keys_len` is the LOGICAL property count;
// `field_count` is PHYSICAL — capped at the object's inline slot allocation.
// An object grown by name past INLINE_SLOT_FLOOR (4) keeps `field_count`
// pinned at the floor and parks the rest of its values in overflow storage.
//
// `JSON.parse`'s lazy tape materializer builds exactly that shape, so
// `JSON.stringify(JSON.parse(blob))` silently dropped every property past the
// 4th from EVERY record, as soon as any property of any element was read
// (the read is what forces materialization). Five-field API records — the
// single most common JSON shape there is — lost a field with no diagnostic.
// (#7264; latent since v0.5.65, exposed by #6712 lowering the floor 8 → 4.)
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// (1) Field-count sweep across the inline floor. The truncation was invisible
//     at <= 4 fields and produced IDENTICAL output for 5, 6 and 8 — the tell
//     that emission stopped at the floor.
for (const n of [1, 2, 3, 4, 5, 6, 8, 12]) {
  const items: any[] = [];
  for (let i = 0; i < 32; i++) {
    const o: any = {};
    for (let f = 0; f < n; f++) o["f" + f] = i + f;
    items.push(o);
  }
  const parsed = JSON.parse(JSON.stringify(items));
  const touched = parsed[0]["f0"]; // ONE read forces materialization
  const out = JSON.stringify(parsed);
  console.log(n + " " + touched + " " + out.length + " " + out.slice(0, 72));
}

// (2) Reading ONE property of ONE element corrupted all 32 records; so did a
//     loop over every element; the untouched array was correct. All three
//     must agree.
function sixField(): any[] {
  const items: any[] = [];
  for (let i = 0; i < 32; i++) {
    items.push({ f0: i, f1: i + 1, f2: i + 2, f3: i + 3, f4: i + 4, f5: i + 5 });
  }
  return items;
}
const blob = JSON.stringify(sixField());

const untouched = JSON.parse(blob);
console.log("untouched " + JSON.stringify(untouched).length);

const oneRead = JSON.parse(blob);
const single = oneRead[0].f0;
console.log("one-read " + single + " " + JSON.stringify(oneRead).length);

const loopRead = JSON.parse(blob);
let sum = 0;
for (let i = 0; i < loopRead.length; i++) sum += loopRead[i].f2;
console.log("loop-read " + sum + " " + JSON.stringify(loopRead).length);

console.log("agree " + (JSON.stringify(untouched) === blob) +
  " " + (JSON.stringify(oneRead) === blob) +
  " " + (JSON.stringify(loopRead) === blob));

// (3) The whole-array result must agree with the per-element results and with
//     Object.keys. Only the ARRAY path was wrong, which is what made the bug
//     so hard to see: every element-level probe reported the truth.
const probe = JSON.parse(blob);
const z = probe[0].f0;
console.log("keys " + Object.keys(probe[0]).length + " " + z);
console.log("element " + JSON.stringify(probe[0]));
console.log("mapped " + (JSON.stringify(probe) === "[" + probe.map((o: any) => JSON.stringify(o)).join(",") + "]"));

// (4) The originally-reported shape: the 5th property is a nested object and
//     the 4th an array, so the dropped field was a whole subtree.
const records: any[] = [];
for (let i = 0; i < 32; i++) {
  records.push({
    id: i,
    name: "item_" + i,
    value: i * 3,
    tags: ["tag_" + (i % 10), "tag_" + (i % 5)],
    nested: { x: i, y: i * 2 },
  });
}
const nestedBlob = JSON.stringify(records);
const nestedParsed = JSON.parse(nestedBlob);
let xs = 0;
for (let i = 0; i < nestedParsed.length; i++) xs += nestedParsed[i].nested.x;
console.log("nested " + xs + " " + (JSON.stringify(nestedParsed) === nestedBlob));
console.log("nested-tail " + JSON.stringify(nestedParsed).slice(-64));

// (5) The template's non-primitive fallbacks must still fire for overflow
//     slots: `undefined` is skipped, a function is skipped, and a `toJSON`
//     found past the floor replaces the whole element.
const mixed = JSON.parse(blob);
const mixedZ = mixed[0].f0;
mixed[1].f5 = undefined;
mixed[2].f4 = function ignored() {};
mixed[3].toJSON = function () { return "replaced"; };
console.log("mixed " + mixedZ + " " + JSON.stringify(mixed.slice(0, 5)));

// (6) The opposite skew must not regress: an object holding FEWER properties
//     than its inline allocation must still stop at its last real key.
const sparse: any[] = [];
for (let i = 0; i < 32; i++) sparse.push({ a: i, b: i + 1 });
const sparseParsed = JSON.parse(JSON.stringify(sparse));
const sparseZ = sparseParsed[0].a;
console.log("sparse " + sparseZ + " " + JSON.stringify(sparseParsed).slice(0, 40));

// (7) Heterogeneous array: element 0 seeds the template, later elements have a
//     different shape and must fall back per element rather than being forced
//     through the template.
const hetero = JSON.parse(
  '[{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6},{"a":1,"b":2},{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7}]',
);
const heteroZ = hetero[0].a;
console.log("hetero " + heteroZ + " " + JSON.stringify(hetero));
