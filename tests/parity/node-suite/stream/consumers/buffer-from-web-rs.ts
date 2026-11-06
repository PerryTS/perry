import { ReadableStream } from "node:stream/web";
import { buffer } from "node:stream/consumers";
// buffer() works on Web ReadableStream.
const rs = new ReadableStream({
  start(c) { c.enqueue("ab"); c.enqueue("cd"); c.close(); },
});
const buf = await buffer(rs as any);
console.log("isBuffer:", Buffer.isBuffer(buf));
console.log("content:", buf.toString("utf8"));
