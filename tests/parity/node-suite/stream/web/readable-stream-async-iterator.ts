import { ReadableStream } from "node:stream/web";
// Web ReadableStream supports `for await` since Node 17.
const rs = new ReadableStream({
  start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); },
});
const out: string[] = [];
for await (const v of rs as any) out.push(String(v));
console.log("joined:", out.join(","));
