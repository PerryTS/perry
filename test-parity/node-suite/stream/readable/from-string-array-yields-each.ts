import { Readable } from "node:stream";
// Readable.from(['a', 'b', 'c']) — each string is a separate chunk.
const r = Readable.from(["a", "b", "c"]);
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => {
  console.log("count:", out.length);
  console.log("first:", out[0]);
});
