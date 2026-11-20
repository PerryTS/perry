import { Readable } from "node:stream";
// Readable.from(Promise.resolve(value)) — treats Promise as iterable? No;
// Node yields the resolved value as a single chunk.
const r = Readable.from(Promise.resolve("resolved-value"));
const out: any[] = [];
r.on("data", (v) => out.push(v));
r.on("end", () => {
  console.log("count:", out.length);
  console.log("first:", out[0]);
});
