import { ReadableStream } from "node:stream/web";
// ReadableStream.values(options) returns an async iterator, optionally
// configurable via { preventCancel } (separate from getReader).
const rs = new ReadableStream({
  start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); },
});
const out: string[] = [];
for await (const v of (rs as any).values()) {
  out.push(String(v));
}
console.log("values:", out.join(","));
