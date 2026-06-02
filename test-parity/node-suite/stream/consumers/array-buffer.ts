import { Buffer } from "node:buffer";
import { Readable } from "node:stream";
import { arrayBuffer } from "node:stream/consumers";

const directArrayBuffer = new Uint8Array([122, 33]).buffer;
const value = await arrayBuffer(
  Readable.from([Buffer.from("xy"), directArrayBuffer]),
);
const bytes = new Uint8Array(value);
console.log("isArrayBuffer:", value instanceof ArrayBuffer);
console.log("byteLength:", value.byteLength);
console.log("first:", bytes[0]);
console.log("second:", bytes[1]);
console.log("third:", bytes[2]);
console.log("fourth:", bytes[3]);
