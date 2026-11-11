import { ReadableStream } from "node:stream/web";
// RS.from(Uint16Array) — typed array is iterable; yields each element.
const arr = new Uint16Array([100, 200, 300]);
const rs = (ReadableStream as any).from(arr);
const reader = rs.getReader();
const out: number[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(value as number);
}
console.log("count:", out.length);
console.log("values:", out.join(","));
