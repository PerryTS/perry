import { Readable, PassThrough } from "node:stream";
// pipe(dst, {end:false}) — manual dst.end() works after source ends.
const r = Readable.from(["a"]);
const dst = new PassThrough();
const out: string[] = [];
dst.on("data", (c) => out.push(String(c)));
dst.on("end", () => console.log("dst ended:", out.join(",")));
r.pipe(dst, { end: false });
r.on("end", () => {
  setImmediate(() => {
    dst.end();
  });
});
