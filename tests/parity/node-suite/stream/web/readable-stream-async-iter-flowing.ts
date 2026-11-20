import { ReadableStream } from "node:stream/web";
// Web RS supports Symbol.asyncIterator — iteration drains all chunks.
const rs = new ReadableStream({
  start(c) {
    c.enqueue("p");
    c.enqueue("q");
    c.close();
  },
});
const out: string[] = [];
for await (const v of rs as any) out.push(String(v));
console.log("collected:", out.join(","));
