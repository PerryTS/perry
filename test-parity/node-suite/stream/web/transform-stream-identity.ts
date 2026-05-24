import { TransformStream } from "node:stream/web";
// `new TransformStream()` with no transformer is identity (passthrough):
// values written to writable appear unchanged on readable.
const ts = new TransformStream();
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("a");
await writer.write("b");
await writer.close();
const out: string[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(String(value));
}
console.log("identity passthrough:", out.join(","));
