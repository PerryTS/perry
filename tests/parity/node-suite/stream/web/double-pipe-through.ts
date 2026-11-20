import { ReadableStream, TransformStream } from "node:stream/web";
// Chain pipeThrough() twice — two transforms in sequence.
const rs = new ReadableStream({ start(c) { c.enqueue("ab"); c.close(); } });
const upper = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(String(c).toUpperCase()); },
});
const exclaim = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(String(c) + "!"); },
});
const result = rs.pipeThrough(upper).pipeThrough(exclaim);
const reader = result.getReader();
const { value } = await reader.read();
console.log("result:", value);
