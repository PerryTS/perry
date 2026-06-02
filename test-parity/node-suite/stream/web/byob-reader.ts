import { ReadableStream } from "node:stream/web";
// getReader({ mode: 'byob' }) returns a BYOB reader for byte streams.
const rs = new ReadableStream({
  type: "bytes",
  start(c: any) { c.enqueue(new Uint8Array([1, 2])); c.close(); },
});
const reader = (rs as any).getReader({ mode: "byob" });
console.log("has byobRead:", typeof reader.read === "function");
