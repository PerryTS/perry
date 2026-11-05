import { TransformStream } from "node:stream/web";
// A TransformStream with empty writable.close() yields an empty readable.
const ts = new TransformStream();
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.close();
const { value, done } = await reader.read();
console.log("value:", value);
console.log("done:", done);
