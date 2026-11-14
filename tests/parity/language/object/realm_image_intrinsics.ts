// Reflection observes held descriptors and symbol identities before using them.
const mapProto: any = Map.prototype;
const setProto: any = Set.prototype;
const arrayProto: any = Array.prototype;
const typedProto: any = Uint8Array.prototype;
console.log(Object.getPrototypeOf(Buffer.prototype) === typedProto);
console.log(Object.getPrototypeOf(typedProto) === Object.getPrototypeOf(Int8Array.prototype));
console.log(mapProto[Symbol.iterator] === mapProto.entries);
console.log(setProto[Symbol.iterator] === setProto.values);
console.log(arrayProto[Symbol.iterator] === arrayProto.values);
for (const holder of [mapProto, setProto, arrayProto]) {
    const descriptor = Object.getOwnPropertyDescriptor(holder, Symbol.iterator);
    console.log(typeof descriptor.value, descriptor.writable, descriptor.enumerable, descriptor.configurable);
    const symbols = Object.getOwnPropertySymbols(holder);
    console.log(symbols.indexOf(Symbol.iterator) >= 0);
}
console.log(new Map([[1, 2]]).get(1), new Set([3]).has(3));
console.log(Object.getPrototypeOf(function* () {}) === Object.getPrototypeOf(function* () {}));
console.log(Object.getPrototypeOf((function* () {})()) === (function* () {}).prototype);
console.log(Object.getPrototypeOf(BigInt) === Function.prototype);
