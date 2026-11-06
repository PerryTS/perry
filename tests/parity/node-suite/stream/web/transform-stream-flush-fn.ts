import { TransformStream } from "node:stream/web";
// TransformStream's flush() (transformer.flush) runs before the readable
// closes — can enqueue a final chunk.
const ts = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(c); },
  flush(ctrl) { ctrl.enqueue("FINAL"); },
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("x");
await writer.close();
const a = await reader.read();
const b = await reader.read();
const c = await reader.read();
console.log("a:", a.value);
console.log("b:", b.value);
console.log("c done:", c.done);
