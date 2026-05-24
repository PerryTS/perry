import { WritableStream } from "node:stream/web";
// The sink's close() can return a Promise that delays writer.close() resolution.
let closed = false;
const ws = new WritableStream({
  write() {},
  async close() {
    await new Promise((resolve) => setTimeout(resolve, 10));
    closed = true;
  },
});
const w = ws.getWriter();
await w.write("x");
await w.close();
console.log("close ran (async):", closed);
