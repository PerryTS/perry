import { Readable } from "node:stream";
// Readable.from(Buffer) — Node short-circuits and emits the whole buffer
// as a single chunk.
const r = Readable.from(Buffer.from("abc"));
const chunks: any[] = [];
r.on("data", (c) => chunks.push(c));
r.on("end", () => {
  console.log("count:", chunks.length);
  console.log("first is buffer:", Buffer.isBuffer(chunks[0]));
});
