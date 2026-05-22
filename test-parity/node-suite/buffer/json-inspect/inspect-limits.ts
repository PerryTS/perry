import { Buffer, INSPECT_MAX_BYTES } from "node:buffer";

console.log("inspect max type:", typeof INSPECT_MAX_BYTES, INSPECT_MAX_BYTES > 0);
console.log("small inspect:", Buffer.from([1, 2, 3]).inspect());
console.log("toString tag:", Object.prototype.toString.call(Buffer.from([])));
