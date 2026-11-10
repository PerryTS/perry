import { Readable } from "node:stream";
// Readable.from("") — empty string is iterable but yields nothing.
const r = Readable.from("");
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => {
  console.log("count:", out.length);
  console.log("is empty:", out.length === 0);
});
