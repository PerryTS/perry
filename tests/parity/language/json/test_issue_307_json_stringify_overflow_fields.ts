// Closes #307: JSON.stringify(parseResult) returned "null" for objects with ≥9
// fields. Two interacting bugs: (1) `is_object_pointer` gated stringify on
// `keys_len <= field_count`, but parser-built objects with ≥9 fields cap
// field_count at the inline alloc_limit (8) and store overflow values in the
// OVERFLOW_FIELDS HashMap — keys_len=9, field_count=8 fell through to the
// "null" else-branch. (2) Even past that gate, `actual_fields = min(num_fields,
// keys_len)` and the shape-template fast path's similar min would only emit 8
// fields. Fix routes overflow reads through `js_object_get_field`'s
// overflow_get fallback and skips the shape-template path for overflow objects.

console.log("=== object literals (control: works at all sizes) ===");
const lit9 = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9 };
console.log(JSON.stringify(lit9));
const lit12 = {
  a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10, k: 11, l: 12,
};
console.log(JSON.stringify(lit12));

console.log("=== JSON.parse → JSON.stringify roundtrip ===");
const src9 = '{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":9}';
const round9 = JSON.parse(src9);
console.log(JSON.stringify(round9));

const src12 =
  '{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":9,"j":10,"k":11,"l":12}';
const round12 = JSON.parse(src12);
console.log(JSON.stringify(round12));

console.log("=== boundary (8 fields — the inline-slot cap) ===");
const src8 = '{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8}';
const round8 = JSON.parse(src8);
console.log(JSON.stringify(round8));

console.log("=== mixed value types in overflow ===");
const srcMixed =
  '{"a":1,"b":"hello","c":true,"d":null,"e":3.14,"f":"world","g":42,"h":false,"i":["x","y"]}';
const roundMixed = JSON.parse(srcMixed);
console.log(JSON.stringify(roundMixed));

console.log("=== dynamic obj[k] = v construction past inline cap ===");
const dyn = {} as Record<string, number>;
for (let i = 0; i < 12; i++) {
  dyn["field_" + i] = i;
}
console.log(JSON.stringify(dyn));

console.log("=== nested overflow objects ===");
const nested = JSON.parse(
  '{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":{"x":1,"y":2,"z":3,"w":4,"v":5,"u":6,"t":7,"s":8,"r":9}}'
);
console.log(JSON.stringify(nested));

console.log("=== field access on overflow objects works ===");
const accessed = JSON.parse(
  '{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":9,"j":10}'
);
console.log("a=" + accessed.a + " i=" + accessed.i + " j=" + accessed.j);
