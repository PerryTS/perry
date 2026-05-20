import * as util from "node:util";
const isUint16Array = util.types.isUint16Array;
const isInt32Array = util.types.isInt32Array;
const isFloat64Array = util.types.isFloat64Array;
console.log("uint16:", isUint16Array(new Uint16Array(2)));
console.log("int32:", isInt32Array(new Int32Array(2)));
console.log("float64:", isFloat64Array(new Float64Array(2)));
