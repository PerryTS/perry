import { Readable } from "node:stream";
import { buffer } from "node:stream/consumers";

const buf = await buffer(Readable.from([
  Buffer.from([7, 8]),
  new Uint8Array([9]),
]));

const iter = buf[Symbol.iterator]();
console.log("array:", Array.from(buf).join(","));
console.log("iter:", iter.next().value, iter.next().value, iter.next().value, iter.next().done);
