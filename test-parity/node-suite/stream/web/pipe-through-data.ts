import { ReadableStream, TransformStream } from "node:stream/web";
// pipeThrough() passes data through a TransformStream, returning the
// readable side of the transform with values transformed.
const rs = new ReadableStream({
  start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); },
});
const upper = new TransformStream({
  transform(chunk, ctrl) { ctrl.enqueue(String(chunk).toUpperCase()); },
});
const result = rs.pipeThrough(upper);
const reader = result.getReader();
const out: string[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(String(value));
}
console.log("piped:", out.join(","));
