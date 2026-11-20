// #4139 — the built-in namespace objects (`Math`, `JSON`, `Reflect`) expose
// their own members (methods + constants) to the reflection APIs, and the
// namespace identifier used as a VALUE resolves to the real namespace object
// rather than `globalThis`.
//
// Before #4139 a bare `Math` lowered to the `globalThis` sentinel, so
// `Object.getOwnPropertyDescriptor(Math, "abs")` reflected `globalThis`
// (returning `undefined`) and `Math === globalThis` held. The intrinsic call
// sites (`Math.max(...)`, `JSON.stringify(...)`, `Reflect.get(...)`) are
// AST-gated codegen paths and are exercised here too to prove they still work.

// Identity: the namespace value is the real object, not globalThis.
console.log("typeof", typeof Math, typeof JSON, typeof Reflect);
console.log("Math===globalThis.Math", Math === globalThis.Math);
console.log("JSON===globalThis.JSON", JSON === globalThis.JSON);
console.log("Reflect===globalThis.Reflect", Reflect === globalThis.Reflect);
console.log("Math===globalThis", (Math as any) === (globalThis as any));

// Namespaces are plain objects: no own `name`/`prototype`.
console.log("Math.name", (Math as any).name);
console.log("Math.prototype", (Math as any).prototype);

function memberDesc(o: any, label: string, keys: string[]) {
  console.log("== " + label + " ==");
  for (const k of keys) {
    const d = Object.getOwnPropertyDescriptor(o, k);
    if (d === undefined) {
      console.log(k, "undefined");
    } else if (typeof d.value === "function") {
      console.log(k, "fn", d.value.length, d.writable, d.enumerable, d.configurable);
    } else {
      console.log(k, "data", d.value, d.writable, d.enumerable, d.configurable);
    }
  }
}

memberDesc(Math, "Math", ["abs", "max", "atan2", "random", "PI", "E", "SQRT2", "f16round", "nope"]);
memberDesc(JSON, "JSON", ["parse", "stringify", "rawJSON", "isRawJSON", "nope"]);
memberDesc(Reflect, "Reflect", ["get", "set", "has", "ownKeys", "defineProperty", "nope"]);

// Reflection enumerates own members; they are non-enumerable so Object.keys
// stays empty and the `in` operator sees them.
console.log("Math names count", Object.getOwnPropertyNames(Math).length);
console.log("JSON names", Object.getOwnPropertyNames(JSON).join(","));
console.log("Reflect names count", Object.getOwnPropertyNames(Reflect).length);
console.log("Object.keys(Math)", JSON.stringify(Object.keys(Math)));
console.log("'abs' in Math", "abs" in Math, "'PI' in Math", "PI" in Math);
console.log("globalThis own has abs", Object.getOwnPropertyNames(globalThis).includes("abs"));

// The intrinsic call / constant-fold paths are unchanged.
console.log("Math.max(1,5,3)", Math.max(1, 5, 3));
console.log("Math.abs(-7)", Math.abs(-7));
console.log("Math.floor(3.7)", Math.floor(3.7));
console.log("Math.PI", Math.PI);
console.log("JSON.stringify", JSON.stringify({ a: 1, b: [2, 3] }));
console.log("JSON.parse", JSON.parse("[1,2,3]")[2]);
const o = { x: 42 };
console.log("Reflect.get", Reflect.get(o, "x"));
console.log("Reflect.has", Reflect.has(o, "x"));
console.log("Reflect.ownKeys", JSON.stringify(Reflect.ownKeys(o)));

// A namespace value passed through a local binding keeps reflecting the object.
const M = Math;
console.log("aliased getOwnPropertyDescriptor", JSON.stringify(Object.getOwnPropertyDescriptor(M, "PI")));

// Local shadowing wins over the global namespace.
{
  const Math = { custom: 1 };
  console.log("shadowed custom", (Math as any).custom);
  console.log("shadowed max desc", Object.getOwnPropertyDescriptor(Math, "max"));
}
