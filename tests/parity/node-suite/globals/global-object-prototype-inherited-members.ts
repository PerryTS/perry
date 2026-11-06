// Regression (#6652, pi wall #6): bare identifiers that resolve to
// Object.prototype-INHERITED members of the global object must work exactly
// as in Node — the global scope chain ends at globalThis, whose prototype
// chain reaches Object.prototype, so `hasOwnProperty`, `toString`,
// `valueOf`, ... are all resolvable bare identifiers.
//
// Pre-fix, perry's unknown-identifier-assume-global lowering collapsed the
// ident in member-object position to the globalThis sentinel itself, so
// `hasOwnProperty.call(o, k)` dispatched `.call` against globalThis
// (undefined -> "TypeError: value is not a function"). Trigger in the wild:
// @babel/types/lib/definitions/placeholders.js
// (`hasOwnProperty.call(o, t4) || (o[t4] = [])`) during pi-bundle init.
//
// Receiver semantics (verified against node v26): a bare CALL of such an
// identifier gets `this = undefined` — the global environment record's
// WithBaseObject is undefined — in BOTH module (strict) and sloppy CJS
// scope. Builtins do not coerce: `toString()` is "[object Undefined]" and
// `hasOwnProperty("x")` throws "Cannot convert undefined or null to object".

const o: any = { x: 1 };

// 1. member access on the bare inherited ident (the @babel/types shape)
console.log("call:", hasOwnProperty.call(o, "x"), hasOwnProperty.call(o, "y"));

// 2. bare read: extraction preserves identity with Object.prototype
const h: any = hasOwnProperty;
console.log("read typeof:", typeof h);
console.log("identity:", h === Object.prototype.hasOwnProperty);
console.log("extracted:", h.call({ y: 2 }, "y"), h.call({ y: 2 }, "z"));

// 3. typeof on the bare ident (no extraction)
console.log("typeof:", typeof hasOwnProperty, typeof isPrototypeOf);

// 4. bare toString() called without receiver: this = undefined
console.log("toString():", String(toString()));

// 5. bare call of hasOwnProperty: this = undefined -> ToObject throws
try {
  (hasOwnProperty as any)("x");
  console.log("bare-call: no throw");
} catch (e: any) {
  console.log("bare-call threw:", e.constructor.name + ": " + e.message);
}

// 6. valueOf() without receiver: same ToObject(undefined) throw
try {
  (valueOf as any)();
  console.log("valueOf: no throw");
} catch (e: any) {
  console.log("valueOf threw:", e.constructor.name + ": " + e.message);
}

// 7. other Object.prototype members reached the same way
console.log("isPrototypeOf:", isPrototypeOf === Object.prototype.isPrototypeOf);
console.log(
  "propertyIsEnumerable:",
  propertyIsEnumerable === Object.prototype.propertyIsEnumerable,
);

// 8. from inside a function body (the pi bundle hits this in webpack
//    factories, not at top level)
function usesInherited(obj: any, key: string): boolean {
  return hasOwnProperty.call(obj, key);
}
console.log("in-function:", usesInherited({ k: 0 }, "k"), usesInherited({}, "k"));

// 9. the same by-name runtime resolution must serve runtime-CREATED globals
//    in member position (the ident is invisible to compile-time resolution)
(globalThis as any).issue6652RuntimeGlobal = { prop: 42, greet: () => "hi" };
// @ts-ignore -- deliberately unresolvable at compile time
console.log("runtime-global:", issue6652RuntimeGlobal.prop, issue6652RuntimeGlobal.greet());

// 10. a genuinely missing ident in member position is still the spec
//     ReferenceError, localized to the identifier
try {
  // @ts-ignore -- deliberately unresolvable
  issue6652NeverDefined.foo;
  console.log("missing: no throw");
} catch (e: any) {
  console.log("missing threw:", e.constructor.name + ": " + e.message);
}
