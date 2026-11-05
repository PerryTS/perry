import { ReadableStream } from "node:stream/web";
// releaseLock() immediately after getReader() (no read) — stream unlocks.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const reader = rs.getReader();
reader.releaseLock();
console.log("locked after release:", rs.locked);
// Should be able to get another reader
const reader2 = rs.getReader();
const { value, done } = await reader2.read();
console.log("re-read value:", value, "done:", done);
