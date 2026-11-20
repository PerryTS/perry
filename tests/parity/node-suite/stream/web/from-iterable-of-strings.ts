import { ReadableStream } from "node:stream/web";
// RS.from(stringArray) — each string is a chunk.
const rs = (ReadableStream as any).from(["alpha", "beta", "gamma"]);
const reader = rs.getReader();
const out: string[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(String(value));
}
console.log("count:", out.length);
console.log("joined:", out.join(","));
