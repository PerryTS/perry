import * as util from "node:util";
const checks = [
  typeof util.types.isArrayBuffer,
  typeof util.types.isTypedArray,
  typeof util.types.isMap,
  typeof util.types.isSet,
  typeof util.types.isPromise,
];
console.log("types functions:", checks.join(","));
