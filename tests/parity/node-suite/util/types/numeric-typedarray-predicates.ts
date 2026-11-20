import { types } from "node:util";
import { isFloat32Array, isInt16Array } from "node:util/types";

const isUint32Array = types.isUint32Array;

console.log("int8:", types.isInt8Array(new Int8Array(1)));
console.log("int8 false:", types.isInt8Array(new Uint16Array(1)));
console.log("int16:", isInt16Array(new Int16Array(1)));
console.log("uint32 ref:", isUint32Array(new Uint32Array(1)));
console.log("float32:", isFloat32Array(new Float32Array(1)));
console.log("clamped:", types.isUint8ClampedArray(new Uint8ClampedArray(1)));
console.log("clamped false:", types.isUint8ClampedArray(new Uint8Array([1])));
