import { ReadableStream, TransformStream } from "node:stream/web";
// pipeThrough — data flows through; collect the result.
const rs = new ReadableStream({
  start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); },
});
const upper = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(String(c).toUpperCase()); },
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
