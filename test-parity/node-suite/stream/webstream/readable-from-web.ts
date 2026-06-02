import { Readable } from "node:stream";
// Readable.fromWeb wraps a WHATWG ReadableStream as a node Readable.
const web = new ReadableStream({
  start(c) {
    c.enqueue("x");
    c.close();
  },
});
const r = (Readable as any).fromWeb(web);
console.log("is Readable:", r instanceof Readable);
