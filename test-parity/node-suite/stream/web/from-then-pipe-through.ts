import { ReadableStream, TransformStream } from "node:stream/web";
// RS.from(arr).pipeThrough(transform) chain
const rs = (ReadableStream as any).from(["a", "b"]);
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
