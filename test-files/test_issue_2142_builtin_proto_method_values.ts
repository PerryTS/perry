// Issue #2142: extension of #2058 to all built-in prototypes.
// Native methods on Array/Date/RegExp/String/Promise/TypedArray prototypes
// returned `undefined` when accessed AS PROPERTY VALUES via the prototype
// object. The method-call form (`arr.map(fn)`) already worked because
// codegen special-cases NativeMethodCall; only the value-read form was
// missing.
//
// The bug was twofold:
//   1. `<Ctor>.prototype` collapsed to `globalThis.prototype` (undefined)
//      for every constructor except typed arrays. After the fix it routes
//      through `globalThis.<Ctor>.prototype` (the real proto object).
//   2. The proto object only had `Array.prototype.slice` and
//      `Object.prototype.toString` installed; the rest of the well-known
//      methods were missing. They're now installed as named callable
//      closures.
//
// The repro is the issue's exact iteration form (the indirect typeof path,
// which the AST-level `typeof <Ctor>.prototype.<m>` fold does NOT cover).

for (var [n, v] of [
  ["Array.prototype.map",        Array.prototype.map],
  ["Array.prototype.filter",     Array.prototype.filter],
  ["Array.prototype.reduce",     Array.prototype.reduce],
  ["Array.prototype.forEach",    Array.prototype.forEach],
  ["Date.prototype.toISOString", Date.prototype.toISOString],
  ["Date.prototype.getTime",     Date.prototype.getTime],
  ["Date.prototype.getFullYear", Date.prototype.getFullYear],
  ["RegExp.prototype.test",      RegExp.prototype.test],
  ["RegExp.prototype.exec",      RegExp.prototype.exec],
  ["String.prototype.slice",     String.prototype.slice],
  ["String.prototype.split",     String.prototype.split],
  ["String.prototype.charAt",    String.prototype.charAt],
  ["Promise.prototype.then",     Promise.prototype.then],
  ["Promise.prototype.catch",    Promise.prototype.catch],
  ["Promise.prototype.finally",  Promise.prototype.finally],
  ["Int8Array.prototype.copyWithin", Int8Array.prototype.copyWithin],
  ["Uint8Array.prototype.map",   Uint8Array.prototype.map],
  ["Float64Array.prototype.set", Float64Array.prototype.set],
  ["Map.prototype.get",          Map.prototype.get],
  ["Set.prototype.add",          Set.prototype.add],
  ["Boolean.prototype.valueOf",  Boolean.prototype.valueOf],
  ["Number.prototype.toFixed",   Number.prototype.toFixed],
]) console.log(n, typeof v);

// Each reified value carries its method name (introspection: Test262 reads
// `.name` on built-in methods via `assert.throws` / `verifyProperty`).
console.log((Array.prototype.map as any).name);
console.log((Date.prototype.getTime as any).name);
console.log((RegExp.prototype.test as any).name);
console.log((Promise.prototype.then as any).name);
console.log((Map.prototype.get as any).name);
