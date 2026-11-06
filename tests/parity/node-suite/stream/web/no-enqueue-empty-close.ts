import { ReadableStream } from "node:stream/web";
// Stream that calls c.close() in start with no enqueue — empty stream.
const rs = new ReadableStream({ start(c) { c.close(); } });
const reader = rs.getReader();
const result = await reader.read();
console.log("value:", result.value);
console.log("done:", result.done);
