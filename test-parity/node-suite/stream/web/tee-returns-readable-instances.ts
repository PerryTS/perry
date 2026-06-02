import { ReadableStream } from "node:stream/web";
// tee() returns a 2-element array of ReadableStream instances.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const result = rs.tee();
console.log("is array:", Array.isArray(result));
console.log("length:", result.length);
console.log("both ReadableStream:", result[0] instanceof ReadableStream && result[1] instanceof ReadableStream);
