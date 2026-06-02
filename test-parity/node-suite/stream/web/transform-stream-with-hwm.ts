import { TransformStream, CountQueuingStrategy } from "node:stream/web";
// TransformStream accepts writableStrategy + readableStrategy with custom
// queuing strategies for fine-grained backpressure control.
const ts = new TransformStream(
  { transform(c, ctl) { ctl.enqueue(c); } },
  new CountQueuingStrategy({ highWaterMark: 2 }),
  new CountQueuingStrategy({ highWaterMark: 1 }),
);
console.log("readable:", ts.readable instanceof ReadableStream);
console.log("writable:", ts.writable instanceof WritableStream);
