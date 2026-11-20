import { Readable, PassThrough } from "node:stream";
// Default autoDestroy:true — after pipe completes, both streams destroyed.
const r = Readable.from(["x"]);
const dst = new PassThrough();
dst.on("data", () => {});
r.pipe(dst);
dst.on("end", () => {
  setImmediate(() => {
    console.log("src destroyed:", r.destroyed);
    console.log("dst destroyed:", dst.destroyed);
  });
});
