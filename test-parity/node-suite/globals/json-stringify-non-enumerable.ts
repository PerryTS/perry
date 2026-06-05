// JSON.stringify serializes only own ENUMERABLE string-keyed properties.
// Non-enumerable own properties (Object.defineProperty with enumerable:false,
// and builtin descriptors like Math's constants or a TypedArray prototype's
// BYTES_PER_ELEMENT) must be skipped — matching Object.keys, across the
// compact, pretty, and function-replacer paths. The array (property-list)
// replacer is the exception: it emits listed keys regardless of enumerability.

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// User-defined non-enumerable own property.
const a: any = { x: 1, y: 2 };
Object.defineProperty(a, "hidden", { value: 9, enumerable: false });
show("compact", JSON.stringify(a));
show("pretty", JSON.stringify(a, null, 2).replace(/\s+/g, " "));
show("fn-replacer", JSON.stringify(a, (_k, v) => v));
show("array-replacer", JSON.stringify(a, ["x", "hidden"])); // array replacer keeps it

// Native objects with non-enumerable own properties.
show("Math", JSON.stringify(Math as any));
show("U8-proto", JSON.stringify(Uint8Array.prototype as any));

// Nested object with a non-enumerable property.
const n: any = { o: {} };
Object.defineProperty(n.o, "hid", { value: 1, enumerable: false });
n.o.vis = 2;
show("nested", JSON.stringify(n));

// Shape-template fast path (>=5 enumerable fields) must still skip a
// non-enumerable sibling.
const big: any = { a: 1, b: 2, c: 3, d: 4, e: 5 };
Object.defineProperty(big, "sec", { value: 6, enumerable: false });
show("big5", JSON.stringify(big));

// A non-enumerable accessor is skipped; freeze leaves enumerability intact.
const g: any = { x: 1 };
Object.defineProperty(g, "ge", { get: () => 7, enumerable: false });
show("getter-nonenum", JSON.stringify(g));
show("frozen", JSON.stringify(Object.freeze({ k: 1 })));

// Regression: descriptor-free objects are unaffected.
show("normal", JSON.stringify({ a: 1, b: [2, 3], c: { d: 4 }, e: "s", f: true, g: null }));
show("array", JSON.stringify([1, { a: 1 }, ["x"]]));
