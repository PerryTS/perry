import { WritableStream } from "node:stream/web";
// close() with pending writes — must wait for the writes to complete
// before the close promise resolves.
let writesProcessed = 0;
const ws = new WritableStream({
  async write() {
    await new Promise((resolve) => setTimeout(resolve, 10));
    writesProcessed++;
  },
});
const w = ws.getWriter();
const a = w.write("a");
const b = w.write("b");
await w.close();
console.log("writes processed:", writesProcessed);
console.log("close resolved after writes:", writesProcessed === 2);
await Promise.all([a, b]);
