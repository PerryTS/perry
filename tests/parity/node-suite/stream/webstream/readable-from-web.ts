import { Readable } from "node:stream";
// Readable.fromWeb wraps a WHATWG ReadableStream as a node Readable and
// forwards readable chunks.
const web = new ReadableStream({
  start(c) {
    c.enqueue("x");
    c.enqueue("y");
    c.close();
  },
});
const r = (Readable as any).fromWeb(web, { objectMode: true });
console.log("is Readable:", r instanceof Readable);
const chunks: string[] = [];
r.on("data", (chunk) => chunks.push(chunk));
await new Promise((resolve) => r.on("end", resolve));
console.log("chunks:", chunks.join(""));
