import { ReadableStream } from "node:stream/web";
// Multiple cancel() calls — second is a no-op (resolves).
const rs = new ReadableStream({ start(c) { c.enqueue("x"); } });
await rs.cancel("first");
const second = await rs.cancel("second");
console.log("second resolved:", second);
console.log("is undefined:", second === undefined);
