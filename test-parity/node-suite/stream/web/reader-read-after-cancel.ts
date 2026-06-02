import { ReadableStream } from "node:stream/web";
// reader.read() after cancel() returns {done: true}.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); } });
const reader = rs.getReader();
await reader.cancel("stop");
const result = await reader.read();
console.log("value:", result.value);
console.log("done:", result.done);
