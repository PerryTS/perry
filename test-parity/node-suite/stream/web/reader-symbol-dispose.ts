import { ReadableStream } from "node:stream/web";
// Web ReadableStreamDefaultReader exposes Symbol.dispose (Node 22+) so it
// can be used with `using` declarations to auto-release the lock.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const reader = rs.getReader();
console.log("has Symbol.dispose:", typeof (reader as any)[Symbol.dispose]);
