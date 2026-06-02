import { Readable } from "node:stream";
// Readable.from("text") — Node short-circuits: single chunk, NOT char-by-char.
const r = Readable.from("text");
const chunks: any[] = [];
r.on("data", (c) => chunks.push(c));
r.on("end", () => {
  console.log("chunks:", chunks.length);
  console.log("first:", chunks[0]);
});
