import { types } from "node:util";

console.log("arraybuffer:", types.isArrayBuffer(new ArrayBuffer(1)));
console.log("sharedarraybuffer:", types.isSharedArrayBuffer(new SharedArrayBuffer(1)));
console.log("anyarraybuffer ab:", types.isAnyArrayBuffer(new ArrayBuffer(1)));
console.log("anyarraybuffer sab:", types.isAnyArrayBuffer(new SharedArrayBuffer(1)));
