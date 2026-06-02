// Issue #462: property access on `undefined` / `null` must throw a
// TypeError, not silently return `undefined`. The silent-undefined
// behavior masked unimplemented-API bugs (a JWS/AES PoC ran to
// completion as a chain of no-ops against an unstubbed
// `crypto.subtle`).
//
// Spec behavior (matches V8 / node):
//   undefined.foo  → "TypeError: Cannot read properties of undefined (reading 'foo')"
//   null.bar       → "TypeError: Cannot read properties of null (reading 'bar')"
//
// Optional chaining (`a?.b`) and explicit null-guards must continue
// to short-circuit — only bare `.` access throws.

const present: any = { value: 42 };
console.log("safe object:", present.value);

const arr: any = [10, 20, 30];
console.log("safe array length:", arr.length);

const undef: any = undefined;
const opt = undef?.foo;
console.log("opt-chain on undefined:", opt);

const guarded: any = undef ?? { fallback: "ok" };
console.log("nullish-coalesce:", guarded.fallback);

const nul: any = null;
const optNull = nul?.bar;
console.log("opt-chain on null:", optNull);

// The next line aborts with a TypeError — execution stops here.
console.log("about to throw — last line of stdout");
console.log(undef.foo);
console.log("UNREACHABLE — should never print");
