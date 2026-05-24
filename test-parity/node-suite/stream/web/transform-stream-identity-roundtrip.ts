import { TransformStream } from "node:stream/web";
// Empty TransformStream is identity — written chunks come out unchanged.
const ts = new TransformStream();
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("hello");
await writer.write("world");
await writer.close();
const out: string[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(String(value));
}
console.log("roundtrip:", out.join("|"));
