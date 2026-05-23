import { Readable } from "node:stream";
import { arrayBuffer } from "node:stream/consumers";
// stream/consumers.arrayBuffer(stream) resolves to an ArrayBuffer.
const r = Readable.from([Buffer.from([1, 2, 3])]);
const ab = await arrayBuffer(r);
console.log("isArrayBuffer:", ab instanceof ArrayBuffer);
console.log("byteLength:", ab.byteLength);
