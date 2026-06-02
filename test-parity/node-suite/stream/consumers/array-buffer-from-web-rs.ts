import { ReadableStream } from "node:stream/web";
import { arrayBuffer } from "node:stream/consumers";
// arrayBuffer() works with Web ReadableStream.
const rs = new ReadableStream({
  start(c) { c.enqueue("abc"); c.close(); },
});
const ab = await arrayBuffer(rs as any);
console.log("is ArrayBuffer:", ab instanceof ArrayBuffer);
console.log("byteLength:", ab.byteLength);
