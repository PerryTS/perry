// JSON.stringify serializes an own accessor property's getter *return value*
// (spec SerializeJSONProperty does an ordinary [[Get]]), not the raw field
// slot — which holds the getter closure (object-literal `get x() {}`) or an
// empty placeholder (`Object.defineProperty(o, k, { get })`). Covers the
// compact, pretty, and function-replacer paths. Non-enumerable accessors are
// skipped; a getter returning undefined omits the key.

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// Object.defineProperty enumerable getter.
const o: any = { x: 1 };
Object.defineProperty(o, "g", { get: () => 7, enumerable: true });
show("defineProperty", JSON.stringify(o));

// Object-literal getter (slot holds the getter closure).
const lit: any = { a: 1, get b() { return 5; }, c: 3 };
show("literal", JSON.stringify(lit));
show("literal-pretty", JSON.stringify(lit, null, 1).replace(/\s+/g, " "));
show("literal-replacer", JSON.stringify(lit, (_k, v) => v));

// Getter returning a nested object/array recurses correctly.
const nest: any = {};
Object.defineProperty(nest, "n", { get: () => ({ z: [1, 2] }), enumerable: true });
show("nested", JSON.stringify(nest));

// Getter returning undefined omits the key.
const u: any = { x: 1 };
Object.defineProperty(u, "u", { get: () => undefined, enumerable: true });
show("undefined", JSON.stringify(u));

// Non-enumerable getter is skipped entirely.
const ne: any = { x: 1 };
Object.defineProperty(ne, "h", { get: () => 9, enumerable: false });
show("nonenum", JSON.stringify(ne));

// The getter runs with the object as `this`.
const t: any = { base: 10, get derived() { return this.base * 2; } };
show("this-binding", JSON.stringify(t));

// Regression: descriptor-free objects are unaffected.
show("normal", JSON.stringify({ a: 1, b: { c: [2, 3] }, d: "x" }));
