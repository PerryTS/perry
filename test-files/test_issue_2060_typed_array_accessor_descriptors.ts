// Issue #2060: accessor (getter) properties on built-in prototypes must be
// reflectable. `Object.getOwnPropertyDescriptor(%TypedArray%.prototype, "length")`
// (reached via `Object.getPrototypeOf(Int8Array.prototype)`) returns a real
// accessor descriptor `{ get, set, enumerable, configurable }` in Node, where
// Perry previously returned `undefined` (the `Cannot read properties of
// undefined (reading 'get')` cascade in the Test262 built-ins/TypedArray sample).

// Exact repro from the issue.
const TAp = Object.getPrototypeOf(Int8Array.prototype);
const d = Object.getOwnPropertyDescriptor(TAp, "length");
console.log(d ? Object.keys(d).sort().join(",") : "(undefined desc)", "| get:", d && typeof d.get);

// Control: a user-defined accessor must keep round-tripping.
const o: any = {};
Object.defineProperty(o, "p", { get() { return 42; }, configurable: true, enumerable: true });
console.log(o.p, Object.getOwnPropertyDescriptor(o, "p").get && "has-get");

// Full descriptor shape for `length`: non-enumerable, configurable, no setter.
console.log("len enumerable:", d!.enumerable, "configurable:", d!.configurable, "set:", d!.set);

// byteLength / byteOffset / buffer are accessor properties too.
for (const k of ["byteLength", "byteOffset", "buffer"]) {
  const dd = Object.getOwnPropertyDescriptor(TAp, k);
  console.log(k, "=>", dd ? ("get:" + typeof dd.get + " enum:" + dd.enumerable + " cfg:" + dd.configurable) : "(undef)");
}

// The getters compute the right value when invoked on a real instance.
const a = new Int8Array(5);
console.log("length.get.call(a):", (d!.get as any).call(a));
const bld = Object.getOwnPropertyDescriptor(TAp, "byteLength");
console.log("byteLength.get.call(a):", (bld!.get as any).call(a));
const i32 = new Int32Array(4);
const bl32 = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int32Array.prototype), "byteLength");
console.log("Int32 byteLength.get.call(i32):", (bl32!.get as any).call(i32));

// Instance property fast-path is unchanged.
console.log("a.length:", a.length, "a.byteLength:", a.byteLength, "a.byteOffset:", a.byteOffset, "a.BYTES_PER_ELEMENT:", a.BYTES_PER_ELEMENT);

// Reflection works across multiple kinds.
const labels = ["Uint8", "Float64", "Uint16"];
const ctors = [Uint8Array, Float64Array, Uint16Array];
for (let i = 0; i < ctors.length; i++) {
  const p = Object.getPrototypeOf((ctors[i] as any).prototype);
  const dl = Object.getOwnPropertyDescriptor(p, "length");
  console.log(labels[i], "length get:", dl && typeof dl.get);
}
