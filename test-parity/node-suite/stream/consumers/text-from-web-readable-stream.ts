import { ReadableStream } from "node:stream/web";
import { text } from "node:stream/consumers";
// text() works on Web ReadableStream too.
const rs = new ReadableStream({
  start(c) { c.enqueue("hello"); c.enqueue(" world"); c.close(); },
});
const result = await text(rs as any);
console.log("result:", result);
console.log("matches:", result === "hello world");
