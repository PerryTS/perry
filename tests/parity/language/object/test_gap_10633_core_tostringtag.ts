// Core intrinsic prototypes expose a fixed Symbol.toStringTag data property.
function describe(name: string, ctor: any, instance: any): void {
  const descriptor = Object.getOwnPropertyDescriptor(ctor.prototype, Symbol.toStringTag);
  console.log(
    name,
    Object.prototype.toString.call(instance),
    String(instance[Symbol.toStringTag]),
    descriptor?.value ?? "MISSING",
    descriptor?.writable ?? "MISSING",
    descriptor?.enumerable ?? "MISSING",
    descriptor?.configurable ?? "MISSING",
  );
}

describe("Map", Map, new Map());
describe("Set", Set, new Set());
describe("WeakMap", WeakMap, new WeakMap());
describe("WeakSet", WeakSet, new WeakSet());
describe("Promise", Promise, Promise.resolve(1));
describe("ArrayBuffer", ArrayBuffer, new ArrayBuffer(4));
describe("DataView", DataView, new DataView(new ArrayBuffer(4)));

console.log("read-only:", Reflect.set(Map.prototype, Symbol.toStringTag, "Other"));
console.log("still Map:", Object.getOwnPropertyDescriptor(Map.prototype, Symbol.toStringTag)?.value);
