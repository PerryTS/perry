import { ReadableStream } from "node:stream/web";
// tee() returns an Array of length 2.
const rs = new ReadableStream({ start(c) { c.close(); } });
const result = rs.tee();
console.log("isArray:", Array.isArray(result));
console.log("length:", result.length);
console.log("[0] is RS:", result[0] instanceof ReadableStream);
console.log("[1] is RS:", result[1] instanceof ReadableStream);
