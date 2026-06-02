import { Readable, PassThrough } from "node:stream";
// pipe(dst, {end:false}) — after src ends, dst is still writable.
const r = Readable.from(["a"]);
const dst = new PassThrough();
const out: string[] = [];
dst.on("data", (c) => out.push(String(c)));
r.pipe(dst, { end: false });
r.on("end", () => {
  setImmediate(() => {
    dst.write("manual");
    dst.end();
  });
});
dst.on("end", () => console.log("out:", out.join(",")));
