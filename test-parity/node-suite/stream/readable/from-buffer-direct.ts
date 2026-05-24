import { Readable } from "node:stream";
// Readable.from(Buffer) — Buffer is iterable so each byte is a chunk in
// objectMode, BUT Node short-circuits and emits whole Buffer as one chunk.
const r = Readable.from(Buffer.from("abc"));
const chunks: any[] = [];
r.on("data", (c) => chunks.push(c));
r.on("end", () => {
  console.log("chunk count:", chunks.length);
  console.log("first chunk is buffer:", Buffer.isBuffer(chunks[0]));
});
