import { Readable, PassThrough } from "node:stream";
// Multiple 'data' listeners on dst — each receives every chunk.
const r = Readable.from(["a", "b"]);
const dst = new PassThrough();
const a: string[] = [];
const b: string[] = [];
dst.on("data", (c) => a.push(String(c)));
dst.on("data", (c) => b.push(String(c)));
r.pipe(dst);
dst.on("end", () => {
  console.log("a:", a.join(","));
  console.log("b:", b.join(","));
});
