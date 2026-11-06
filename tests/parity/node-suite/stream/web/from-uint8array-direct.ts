import { ReadableStream } from "node:stream/web";
// ReadableStream.from(uint8Array) — yields each byte.
const arr = new Uint8Array([10, 20, 30]);
const rs = (ReadableStream as any).from(arr);
const reader = rs.getReader();
const out: number[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(value as number);
}
console.log("collected:", out.join(","));
