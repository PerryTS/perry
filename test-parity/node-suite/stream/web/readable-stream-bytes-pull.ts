import { ReadableStream } from "node:stream/web";
// type:'bytes' ReadableStream with pull() that enqueues a typed-array chunk.
let pulled = 0;
const rs = new ReadableStream({
  type: "bytes",
  pull(c) {
    pulled++;
    if (pulled <= 2) c.enqueue(new Uint8Array([pulled]));
    else c.close();
  },
} as any);
const reader = rs.getReader();
const out: number[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  if (value && (value as Uint8Array).length > 0) out.push((value as Uint8Array)[0]);
}
console.log("pulled:", pulled);
console.log("bytes:", out.join(","));
