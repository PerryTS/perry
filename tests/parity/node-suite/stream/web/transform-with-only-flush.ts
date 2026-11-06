import { TransformStream } from "node:stream/web";
// TransformStream with only flush() (no transform fn) — identity transform
// + flush emits final value.
const ts = new TransformStream({
  flush(c) { c.enqueue("FINAL"); },
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("x");
await writer.close();
const a = await reader.read();
const b = await reader.read();
console.log("a:", a.value);
console.log("b:", b.value);
