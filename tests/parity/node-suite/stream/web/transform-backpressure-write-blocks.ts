import { TransformStream } from "node:stream/web";
// Without reading from the readable, the writable should backpressure.
const ts = new TransformStream({ transform(c, ctrl) { ctrl.enqueue(c); } });
const writer = ts.writable.getWriter();
// Write more than buffer can hold without reading
await writer.write("a");
await writer.write("b");
// desiredSize should be ≤ 0 (backpressured)
console.log("desiredSize:", writer.desiredSize);
console.log("backpressure active:", (writer.desiredSize ?? 0) <= 0);
