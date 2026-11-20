import { ReadableStream } from "node:stream/web";
// ReadableStream.from(arrayOfBuffers) — each element becomes a chunk.
const rs = (ReadableStream as any).from([
  new Uint8Array([1, 2]),
  new Uint8Array([3, 4]),
]);
const reader = rs.getReader();
const out: any[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(value);
}
console.log("count:", out.length);
console.log("first is typed:", out[0] && out[0].constructor && out[0].constructor.name);
