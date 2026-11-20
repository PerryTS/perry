// Issue #5588: Object static methods read AS VALUES (not in call position) —
// e.g. `const f = Object.isExtensible` — collapsed to `undefined` for the
// methods missing from the HIR reification allow-list (isExtensible,
// preventExtensions, setPrototypeOf, seal, isFrozen, isSealed, is,
// defineProperties, getOwnPropertyDescriptors, getOwnPropertySymbols, groupBy).
// They are real native functions in Node, so `typeof` must be "function", the
// `.name`/`.length` own-properties must read, and the bound value must call.

// --- typeof of each static read as a bare value ---
const statics: any = Object;
for (const k of [
  "assign", "create", "defineProperties", "defineProperty", "entries",
  "freeze", "fromEntries", "getOwnPropertyDescriptor",
  "getOwnPropertyDescriptors", "getOwnPropertyNames", "getOwnPropertySymbols",
  "getPrototypeOf", "groupBy", "hasOwn", "is", "isExtensible", "isFrozen",
  "isSealed", "keys", "preventExtensions", "seal", "setPrototypeOf", "values",
]) {
  console.log(k, typeof statics[k]);
}

// --- assigned to a const, then inspected (the failing #5588 shape) ---
const f = Object.isExtensible;
console.log("isExtensible.name:", f.name, "len:", f.length, "typeof:", typeof f);
const s = Object.setPrototypeOf;
console.log("setPrototypeOf.name:", s.name, "len:", s.length);
const p = Object.preventExtensions;
console.log("preventExtensions.name:", p.name, "len:", p.length);

// --- the bound values are actually callable through the alias ---
const target: any = {};
console.log("alias setProto identity:", s(target, { greet() { return "hi"; } }) === target);
console.log("alias greet:", target.greet());
console.log("alias isExtensible:", f(target), f(p({})));

// --- getOwnPropertyDescriptor identity for a static function value ---
const desc = Object.getOwnPropertyDescriptor(Object, "preventExtensions")!;
console.log(
  "desc:", desc.value === Object.preventExtensions,
  desc.writable, desc.enumerable, desc.configurable,
);
