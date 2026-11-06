import { types } from "node:util";

console.log("boxed number:", types.isBoxedPrimitive(new Number(1)));
console.log("uint8:", types.isUint8Array(new Uint8Array(1)));
console.log("int16:", types.isInt16Array(new Int16Array(1)));
console.log("float64:", types.isFloat64Array(new Float64Array(1)));
console.log("bigint64:", types.isBigInt64Array(new BigInt64Array(1)));
console.log("biguint64:", types.isBigUint64Array(new BigUint64Array(1)));
console.log("bigint64 false:", types.isBigInt64Array(new BigUint64Array(1)));
