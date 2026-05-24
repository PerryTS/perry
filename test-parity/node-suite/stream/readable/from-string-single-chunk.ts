import { Readable } from "node:stream";
// Readable.from(string) yields the whole string in a single chunk (not
// character by character) — strings are iterable but Node short-circuits.
const r = Readable.from("hello");
const chunks: string[] = [];
r.on("data", (c) => chunks.push(String(c)));
r.on("end", () => {
  console.log("chunks:", chunks.length);
  console.log("first:", chunks[0]);
});
