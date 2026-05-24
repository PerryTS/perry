import { ReadableStream } from "node:stream/web";
// for-await directly over a Web ReadableStream — should iterate all chunks.
const rs = new ReadableStream({
  start(c) {
    c.enqueue("a");
    c.enqueue("b");
    c.enqueue("c");
    c.close();
  },
});
const out: string[] = [];
for await (const v of rs as any) out.push(String(v));
console.log("collected:", out.join(","));
