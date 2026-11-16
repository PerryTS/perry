// %TypedArray% is one realm intrinsic: the [[Prototype]] of every typed-array
// constructor, with no globalThis binding, its members on its own prototype.
const TA = Object.getPrototypeOf(Int8Array);
const kinds = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array,
  Uint32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array];
console.log(typeof TA, TA.name, TA.length, "TypedArray" in globalThis);
console.log(kinds.every((k) => Object.getPrototypeOf(k) === TA));
console.log(kinds.every((k) => Object.getPrototypeOf(k.prototype) === TA.prototype));
console.log(Object.getPrototypeOf(TA) === Function.prototype, Object.getPrototypeOf(TA.prototype) === Object.prototype);
console.log(Object.getOwnPropertyNames(TA).join(","));
console.log(Object.getOwnPropertyNames(TA.prototype).join(","));
console.log(TA.prototype.constructor === TA, TA.prototype.toString === Array.prototype.toString);
console.log(TA.prototype[Symbol.iterator] === TA.prototype.values, TA[Symbol.species] === TA);
console.log(Int8Array.prototype.hasOwnProperty("map"), Int8Array.prototype.map === TA.prototype.map);
try { TA(); } catch (e) { console.log("call", (e as Error).constructor.name); }
try { new (TA as any)(); } catch (e) { console.log("new", (e as Error).constructor.name, (e as Error).message); }
console.log(TA.from.call(Uint8Array, [1, 2, 3]).join("-"), TA.of.call(Int16Array, 4, 5).join("-"));
