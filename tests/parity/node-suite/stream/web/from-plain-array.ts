import { ReadableStream } from "node:stream/web";
// ReadableStream.from(plainArray) — array is iterable, yields each element.
const rs = (ReadableStream as any).from([10, 20, 30]);
const reader = rs.getReader();
const out: any[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(value);
}
console.log("collected:", out.join(","));
