import { WritableStream, CountQueuingStrategy } from "node:stream/web";
// After a write resolves (sink processed), desiredSize returns to HWM.
const ws = new WritableStream(
  { write() {} },
  new CountQueuingStrategy({ highWaterMark: 2 }),
);
const w = ws.getWriter();
console.log("initial:", w.desiredSize);
await w.write("a");
await w.ready;
console.log("after write:", w.desiredSize);
