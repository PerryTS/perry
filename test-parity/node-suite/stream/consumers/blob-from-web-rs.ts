import { ReadableStream } from "node:stream/web";
import { blob } from "node:stream/consumers";
// blob() works with Web ReadableStream.
const rs = new ReadableStream({
  start(c) { c.enqueue("hi"); c.close(); },
});
const b = await blob(rs as any);
console.log("is Blob:", b instanceof Blob);
console.log("size:", b.size);
