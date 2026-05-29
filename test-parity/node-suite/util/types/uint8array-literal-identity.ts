import { types } from "node:util";

const literal = new Uint8Array(1);
const empty = new Uint8Array();
const dynamicLength = 1;
const dynamic = new Uint8Array(dynamicLength);

console.log("literal uint8:", types.isUint8Array(literal));
console.log("literal typed:", types.isTypedArray(literal));
console.log("literal view:", types.isArrayBufferView(literal));
console.log("empty uint8:", types.isUint8Array(empty));
console.log("dynamic uint8:", types.isUint8Array(dynamic));
console.log("arraybuffer uint8:", types.isUint8Array(new ArrayBuffer(1)));
