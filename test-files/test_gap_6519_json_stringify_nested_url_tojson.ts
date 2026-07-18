// #6519: JSON.stringify of an object/array *containing* a URL instance threw
// `TypeError: Converting circular structure to JSON`. The top-level form is
// intercepted at HIR-lowering time (UrlInstanceToJSON), but a nested URL only
// reaches the runtime tree-walk stringifier, which walked the URL's internal
// ObjectHeader fields and hit the `searchParams` → owner back-reference.
// Node serializes a URL via URL.prototype.toJSON() (its href string), so every
// line below must be byte-identical to node.
const u = new URL("http://x/en", "http://x/");

// URL nested directly in an object / array.
console.log(JSON.stringify({ u }));
console.log(JSON.stringify([u]));

// Pretty-printed (replacer/spacer) forms.
console.log(JSON.stringify({ u }, null, 2));
console.log(JSON.stringify([u], null, 2));

// Two URLs in one array — element 0 is a URL, so the array-of-objects shape
// template must bail and route every element through href serialization.
const v = new URL("http://y/fr?q=1", "http://y/");
console.log(JSON.stringify([u, v]));
console.log(JSON.stringify({ u, v }));

// Deeper nesting.
console.log(JSON.stringify({ list: [{ u }], v }));

// URL carrying populated searchParams — this is the exact back-reference that
// tripped the circular-structure detector.
const w = new URL("http://z/?a=1&b=2");
console.log(JSON.stringify({ w }));
console.log(JSON.stringify([w]));

// Mixed arrays: a plain object builds the shape template first, then a URL
// element (and vice-versa) must fall back to per-element href serialization.
console.log(JSON.stringify([{ a: 1 }, u]));
console.log(JSON.stringify([u, { a: 1 }, v]));

// A replacer that passes values through — toJSON still runs first.
console.log(JSON.stringify({ u }, (_k: string, val: unknown) => val, 2));

// Regression guard: the top-level HIR interception path must still work.
console.log(JSON.stringify(u));
