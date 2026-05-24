import { TransformStream, CountQueuingStrategy } from "node:stream/web";
// new TransformStream(undefined, ws, rs) — Identity with explicit HWMs.
const ws = new CountQueuingStrategy({ highWaterMark: 5 });
const rs = new CountQueuingStrategy({ highWaterMark: 3 });
const ts = new TransformStream(undefined, ws, rs);
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("x");
await writer.close();
const result = await reader.read();
console.log("value:", result.value);
