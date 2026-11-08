// The own keys of %TypedArray%.prototype, Uint8Array, Uint8Array.prototype
// and another kind come in node's order (Reflect.ownKeys reports a built-in's
// insertion order, so the order is observable).
const TA: any = Object.getPrototypeOf(Uint8Array);
const keys = (o: object) => Reflect.ownKeys(o).map(String).join(" ");
console.log(keys(TA.prototype));
console.log(keys(Uint8Array));
console.log(keys(Uint8Array.prototype));
console.log(keys(Float64Array));
console.log(keys(Float64Array.prototype));
console.log(Object.keys(TA.prototype).length, Object.getOwnPropertyNames(Uint8Array.prototype).join(","));
const d = Object.getOwnPropertyDescriptor(Uint8Array, "BYTES_PER_ELEMENT")!;
console.log(d.value, d.writable, d.enumerable, d.configurable);
const p = Object.getOwnPropertyDescriptor(Uint8Array.prototype, "BYTES_PER_ELEMENT")!;
console.log(p.value, p.writable, p.enumerable, p.configurable);
