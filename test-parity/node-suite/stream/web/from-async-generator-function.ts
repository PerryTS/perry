import { ReadableStream } from "node:stream/web";
// ReadableStream.from with an async generator's return value (an iterator).
async function* gen() {
  yield "a";
  yield "b";
}
const rs = (ReadableStream as any).from(gen());
const reader = rs.getReader();
const out: string[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(String(value));
}
console.log("collected:", out.join(","));
