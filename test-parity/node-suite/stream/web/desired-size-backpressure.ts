import { WritableStream } from "node:stream/web";
// writer.desiredSize reflects the slack in the underlying queue:
// HWM - queued bytes. With a count strategy, HWM=1 starts at 1; after a
// pending write it should be 0 or negative.
const ws = new WritableStream(
  {
    async write() {
      await new Promise((resolve) => setTimeout(resolve, 50));
    },
  },
  new (globalThis as any).CountQueuingStrategy({ highWaterMark: 1 }),
);
const w = ws.getWriter();
console.log("initial desiredSize:", w.desiredSize);
w.write("a"); // first write — fills the queue
console.log("after one write desiredSize <= 0:", (w.desiredSize ?? 0) <= 0);
