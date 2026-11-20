// #4276 — the "uncurry-this" idiom
// `Function.prototype.call.bind(Object.prototype.hasOwnProperty)` returned the
// wrong value (an empty `[object Object]`) instead of invoking the method, but
// ONLY when the eventual receiver was a builtin prototype object such as
// `Object.prototype` or `Error.prototype`. Plain objects, `Array.prototype`,
// and `Math` already worked. Root cause: those prototypes carry their own
// `hasOwnProperty` as a thunk that re-dispatches `js_native_call_method(this,
// "hasOwnProperty")`; the dispatcher's own-field scan re-found the field and
// recursed until the call-depth guard bailed, returning the empty-object
// sentinel.
//
// This idiom is the backbone of test262's `propertyHelper.js` `verifyProperty`,
// so it gated every descriptor test subjecting `Object.prototype` /
// `Error.prototype`.

const hasOwn = Function.prototype.call.bind(Object.prototype.hasOwnProperty);

// Plain object + Array.prototype + Math already worked; keep them as anchors.
console.log("plain own", hasOwn({ a: 1 }, "a"));
console.log("plain missing", hasOwn({ a: 1 }, "b"));
console.log("Array.prototype push", hasOwn(Array.prototype, "push"));
console.log("Math abs", hasOwn(Math, "abs"));

// The regressing receivers: builtin prototype objects.
console.log("Object.prototype toString", hasOwn(Object.prototype, "toString"));
console.log("Object.prototype hasOwnProperty", hasOwn(Object.prototype, "hasOwnProperty"));
console.log("Object.prototype missing", hasOwn(Object.prototype, "nope"));
console.log("Error.prototype message", hasOwn(Error.prototype, "message"));
console.log("Error.prototype name", hasOwn(Error.prototype, "name"));
console.log("Error.prototype toString", hasOwn(Error.prototype, "toString"));
console.log("Error.prototype missing", hasOwn(Error.prototype, "zzz"));
console.log("TypeError.prototype name", hasOwn(TypeError.prototype, "name"));
console.log("RangeError.prototype constructor", hasOwn(RangeError.prototype, "constructor"));

// typeof of the result must be "boolean", not "object" (the bug returned the
// empty-object sentinel).
const r = hasOwn(Object.prototype, "toString");
console.log("typeof result", typeof r);

// The same uncurry indirection over the OTHER Object.prototype methods must
// keep working (these never recursed, but guard against the fix over-reaching).
const toStr = Function.prototype.call.bind(Object.prototype.toString);
console.log("uncurry toString array", toStr([]));
console.log("uncurry toString plain", toStr({}));

const isEnum = Function.prototype.call.bind(Object.prototype.propertyIsEnumerable);
console.log("uncurry pie own", isEnum({ a: 1 }, "a"));
console.log("uncurry pie proto", isEnum(Object.prototype, "toString"));

const isProto = Function.prototype.call.bind(Object.prototype.isPrototypeOf);
console.log("uncurry isPrototypeOf", isProto(Object.prototype, {}));

// The direct `.call` form (already worked) must remain correct.
console.log("direct call", Object.prototype.hasOwnProperty.call(Object.prototype, "toString"));
console.log("method form", ({ a: 1 }).hasOwnProperty("a"));
console.log("method form missing", ({ a: 1 }).hasOwnProperty("toString"));
