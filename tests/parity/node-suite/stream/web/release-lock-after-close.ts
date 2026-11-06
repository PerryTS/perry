import { ReadableStream } from "node:stream/web";
// releaseLock after the stream closes — should still work.
const rs = new ReadableStream({ start(c) { c.close(); } });
const reader = rs.getReader();
const { done } = await reader.read();
console.log("done:", done);
reader.releaseLock();
console.log("locked after release:", rs.locked);
